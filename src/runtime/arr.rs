use crate::runtime::types::*;
use crate::wasm::ir::*;

/// Persistent bit-partitioned trie vector with branching factor 32.
///
/// Representation invariants (maintained by all public operations):
///   - 0 <= tail.data.len <= 32
///   - len = trie_element_count + tail.data.len
///   - root = null  iff  len <= 32  (tail-only vector)
///   - shift = 0 when root = null; otherwise shift = 5 * tree_depth
///   - Trie leaves always contain exactly 32 elements
///   - For len > 0, tail.data.len > 0 (the tail is never empty on a non-empty vector)
///
/// These invariants cannot be structurally enforced by the Wasm GC type system
/// (e.g. a PVec with len=5 but an empty tail array is representable), so they
/// are upheld by construction in push, make, set, builder_freeze, etc.
const B: i32 = 5;
const BF: i32 = 1 << B; // 32
const MASK: i32 = BF - 1; // 31

fn ref_eq() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Eq,
    }
}

fn ref_eq_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Eq,
    }
}

/// $PVec field indices
const PV_LEN: u32 = 0;
const PV_SHIFT: u32 = 1;
const PV_ROOT: u32 = 2;
const PV_TAIL: u32 = 3;

/// $VecInternal field indices
const VI_CHILDREN: u32 = 0;
#[allow(dead_code)]
const VI_SIZES: u32 = 1;

/// Build the `rt.arr` module: persistent bit-partitioned trie vector operations.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.arr");

    // ── globals: empty vector singleton ──
    m.globals.push(GlobalDef {
        name: "empty_leaf".into(),
        mutable: false,
        ty: ref_array(),
        init: vec![Instr::ArrayNewFixed(T_ARRAY.into(), 0)],
    });
    m.globals.push(GlobalDef {
        name: "empty_pvec".into(),
        mutable: false,
        ty: ref_pvec(),
        init: vec![
            Instr::I32Const(0),
            Instr::I32Const(0),
            Instr::RefNull(HeapType::Named(T_VEC_INTERNAL.into())),
            Instr::GlobalGet("empty_leaf".into()),
            Instr::StructNew(T_PVEC.into()),
        ],
    });

    // ── internal helpers (not exported by default, but we export everything) ──
    m.funcs.push(tailoff_fn());
    m.funcs.push(get_leaf_fn());
    m.funcs.push(new_path_fn());
    m.funcs.push(push_tail_fn());
    m.funcs.push(do_set_fn());
    m.funcs.push(push_fn());

    // ── public API ──
    m.funcs.push(make_fn());
    m.funcs.push(get_fn());
    m.funcs.push(set_fn());
    m.funcs.push(len_fn());
    m.funcs.push(concat_fn());
    m.funcs.push(slice_fn());
    m.funcs.push(pop_tail_fn());
    m.funcs.push(drop_last_fn());
    m.funcs.push(builder_new_fn());
    m.funcs.push(builder_from_fn());
    m.funcs.push(builder_push_fn());
    m.funcs.push(builder_extend_fn());
    m.funcs.push(builder_freeze_fn());
    m.funcs.push(from_array_fn());
    m.funcs.push(to_array_fn());
    m.funcs.push(from_read_file_result_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

// ═══════════════════════════════════════════════════════════════════════════
// Internal helpers
// ═══════════════════════════════════════════════════════════════════════════

/// `tailoff(len: i32) -> i32`
/// if len <= 32 { 0 } else { ((len - 1) >> 5) << 5 }
fn tailoff_fn() -> FuncDef {
    FuncDef {
        name: "tailoff".into(),
        params: vec![ValType::I32],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::I32Const(BF),
            Instr::I32LeS,
            Instr::If {
                result: Some(ValType::I32),
                then_body: vec![Instr::I32Const(0)],
                else_body: vec![
                    Instr::LocalGet(0),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::I32Const(B),
                    Instr::I32ShrU,
                    Instr::I32Const(B),
                    Instr::I32Shl,
                ],
            },
        ],
    }
}

/// `get_leaf(vec: PVec, idx: i32) -> Array`
/// Navigate trie to find the leaf array containing element at idx.
fn get_leaf_fn() -> FuncDef {
    FuncDef {
        name: "get_leaf".into(),
        params: vec![ref_pvec(), ValType::I32],
        results: vec![ref_array()],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ref_vec_internal_null(),
            ValType::I32,
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(2),
            Instr::LocalGet(2),
            Instr::I32Const(BF),
            Instr::I32LeS,
            Instr::If {
                result: Some(ref_array()),
                then_body: vec![Instr::LocalGet(0), Instr::StructGet(T_PVEC.into(), PV_TAIL)],
                else_body: vec![
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::I32Const(B),
                    Instr::I32ShrU,
                    Instr::I32Const(B),
                    Instr::I32Shl,
                    Instr::LocalSet(3),
                    Instr::LocalGet(1),
                    Instr::LocalGet(3),
                    Instr::I32GeS,
                    Instr::If {
                        result: Some(ref_array()),
                        then_body: vec![
                            Instr::LocalGet(0),
                            Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        ],
                        else_body: {
                            let mut b = vec![
                                Instr::LocalGet(0),
                                Instr::StructGet(T_PVEC.into(), PV_ROOT),
                                Instr::LocalSet(4),
                                Instr::LocalGet(0),
                                Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                                Instr::LocalSet(5),
                                Instr::Block {
                                    label: "brk".into(),
                                    result: None,
                                    body: vec![Instr::Loop {
                                        label: "lp".into(),
                                        result: None,
                                        body: vec![
                                            Instr::LocalGet(5),
                                            Instr::I32Const(B),
                                            Instr::I32LeS,
                                            Instr::BrIf("brk".into()),
                                            Instr::LocalGet(4),
                                            Instr::RefAsNonNull,
                                            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                                            Instr::LocalGet(1),
                                            Instr::LocalGet(5),
                                            Instr::I32ShrU,
                                            Instr::I32Const(MASK),
                                            Instr::I32And,
                                            Instr::ArrayGet(T_VEC_CHILDREN.into()),
                                            Instr::RefCast {
                                                nullable: true,
                                                heap: HeapType::Named(T_VEC_INTERNAL.into()),
                                            },
                                            Instr::LocalSet(4),
                                            Instr::LocalGet(5),
                                            Instr::I32Const(B),
                                            Instr::I32Sub,
                                            Instr::LocalSet(5),
                                            Instr::Br("lp".into()),
                                        ],
                                    }],
                                },
                            ];
                            b.push(Instr::LocalGet(4));
                            b.push(Instr::RefAsNonNull);
                            b.push(Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN));
                            b.push(Instr::LocalGet(1));
                            b.push(Instr::LocalGet(5));
                            b.push(Instr::I32ShrU);
                            b.push(Instr::I32Const(MASK));
                            b.push(Instr::I32And);
                            b.push(Instr::ArrayGet(T_VEC_CHILDREN.into()));
                            b.push(Instr::RefCast {
                                nullable: false,
                                heap: HeapType::Named(T_ARRAY.into()),
                            });
                            b
                        },
                    },
                ],
            },
        ],
    }
}

/// `new_path(level: i32, node: eqref) -> eqref`
/// Wrap node in a chain of VecInternal nodes from level down to 0.
fn new_path_fn() -> FuncDef {
    FuncDef {
        name: "new_path".into(),
        params: vec![ValType::I32, ref_eq()],
        results: vec![ref_eq()],
        locals: vec![ref_vec_children_null()],
        body: vec![
            Instr::Block {
                label: "brk".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "lp".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(0),
                        Instr::I32Eqz,
                        Instr::BrIf("brk".into()),
                        Instr::RefNull(HeapType::Eq),
                        Instr::I32Const(BF),
                        Instr::ArrayNew(T_VEC_CHILDREN.into()),
                        Instr::LocalSet(2),
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::I32Const(0),
                        Instr::LocalGet(1),
                        Instr::ArraySet(T_VEC_CHILDREN.into()),
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::RefNull(HeapType::Named(T_I32_ARRAY.into())),
                        Instr::StructNew(T_VEC_INTERNAL.into()),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Eq,
                        },
                        Instr::LocalSet(1),
                        Instr::LocalGet(0),
                        Instr::I32Const(B),
                        Instr::I32Sub,
                        Instr::LocalSet(0),
                        Instr::Br("lp".into()),
                    ],
                }],
            },
            Instr::LocalGet(1),
        ],
    }
}

/// `push_tail(level: i32, parent: VecInternal, tail_node: eqref) -> eqref`
fn push_tail_fn() -> FuncDef {
    FuncDef {
        name: "push_tail".into(),
        params: vec![
            ValType::I32,
            ValType::I32,
            ref_vec_internal_null(),
            ref_eq(),
        ],
        results: vec![ref_eq()],
        locals: vec![ref_vec_children_null(), ValType::I32, ref_eq_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::LocalGet(1),
            Instr::I32ShrU,
            Instr::I32Const(MASK),
            Instr::I32And,
            Instr::LocalSet(5),
            Instr::RefNull(HeapType::Eq),
            Instr::I32Const(BF),
            Instr::ArrayNew(T_VEC_CHILDREN.into()),
            Instr::LocalSet(4),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(2),
            Instr::RefAsNonNull,
            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
            Instr::I32Const(0),
            Instr::I32Const(BF),
            Instr::ArrayCopy(T_VEC_CHILDREN.into(), T_VEC_CHILDREN.into()),
            Instr::LocalGet(1),
            Instr::I32Const(B),
            Instr::I32Eq,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(5),
                    Instr::LocalGet(3),
                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                ],
                else_body: vec![
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                    Instr::LocalGet(5),
                    Instr::ArrayGet(T_VEC_CHILDREN.into()),
                    Instr::LocalSet(6),
                    Instr::LocalGet(6),
                    Instr::RefIsNull,
                    Instr::If {
                        result: Some(ref_eq()),
                        then_body: vec![
                            Instr::LocalGet(1),
                            Instr::I32Const(B),
                            Instr::I32Sub,
                            Instr::LocalGet(3),
                            Instr::Call("new_path".into()),
                        ],
                        else_body: vec![
                            Instr::LocalGet(0),
                            Instr::LocalGet(1),
                            Instr::I32Const(B),
                            Instr::I32Sub,
                            Instr::LocalGet(6),
                            Instr::RefCast {
                                nullable: true,
                                heap: HeapType::Named(T_VEC_INTERNAL.into()),
                            },
                            Instr::LocalGet(3),
                            Instr::Call("push_tail".into()),
                        ],
                    },
                    Instr::LocalSet(6),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(5),
                    Instr::LocalGet(6),
                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                ],
            },
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::RefNull(HeapType::Named(T_I32_ARRAY.into())),
            Instr::StructNew(T_VEC_INTERNAL.into()),
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Eq,
            },
        ],
    }
}

/// `do_set(level: i32, node: eqref, idx: i32, val: anyref) -> eqref`
fn do_set_fn() -> FuncDef {
    FuncDef {
        name: "do_set".into(),
        params: vec![ValType::I32, ref_eq(), ValType::I32, ValType::Anyref],
        results: vec![ref_eq()],
        locals: vec![
            ref_vec_children_null(),
            ValType::I32,
            ref_array_null(),
            ref_array_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::I32Eqz,
            Instr::If {
                result: Some(ref_eq()),
                then_body: vec![
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_ARRAY.into()),
                    },
                    Instr::LocalSet(7),
                    Instr::RefNull(HeapType::None),
                    Instr::LocalGet(7),
                    Instr::RefAsNonNull,
                    Instr::ArrayLen,
                    Instr::ArrayNew(T_ARRAY.into()),
                    Instr::LocalSet(6),
                    Instr::LocalGet(6),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(7),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(7),
                    Instr::RefAsNonNull,
                    Instr::ArrayLen,
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                    Instr::LocalGet(6),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(2),
                    Instr::I32Const(MASK),
                    Instr::I32And,
                    Instr::LocalGet(3),
                    Instr::ArraySet(T_ARRAY.into()),
                    Instr::LocalGet(6),
                    Instr::RefAsNonNull,
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Eq,
                    },
                ],
                else_body: vec![
                    Instr::LocalGet(2),
                    Instr::LocalGet(0),
                    Instr::I32ShrU,
                    Instr::I32Const(MASK),
                    Instr::I32And,
                    Instr::LocalSet(5),
                    Instr::RefNull(HeapType::Eq),
                    Instr::I32Const(BF),
                    Instr::ArrayNew(T_VEC_CHILDREN.into()),
                    Instr::LocalSet(4),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_VEC_INTERNAL.into()),
                    },
                    Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                    Instr::I32Const(0),
                    Instr::I32Const(BF),
                    Instr::ArrayCopy(T_VEC_CHILDREN.into(), T_VEC_CHILDREN.into()),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(5),
                    Instr::LocalGet(0),
                    Instr::I32Const(B),
                    Instr::I32Sub,
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_VEC_INTERNAL.into()),
                    },
                    Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                    Instr::LocalGet(5),
                    Instr::ArrayGet(T_VEC_CHILDREN.into()),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(2),
                    Instr::LocalGet(3),
                    Instr::Call("do_set".into()),
                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::RefNull(HeapType::Named(T_I32_ARRAY.into())),
                    Instr::StructNew(T_VEC_INTERNAL.into()),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Eq,
                    },
                ],
            },
        ],
    }
}

/// `push(vec: PVec, val: anyref) -> PVec`
/// Append a single element to the vector.
fn push_fn() -> FuncDef {
    FuncDef {
        name: "push".into(),
        params: vec![ref_pvec(), ValType::Anyref],
        results: vec![ref_pvec()],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ref_array_null(),
            ref_eq_null(),
            ValType::I32,
            ref_vec_children_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(2),
            Instr::LocalGet(0),
            Instr::StructGet(T_PVEC.into(), PV_TAIL),
            Instr::ArrayLen,
            Instr::LocalSet(3),
            Instr::LocalGet(3),
            Instr::I32Const(BF),
            Instr::I32LtS,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: vec![
                    Instr::RefNull(HeapType::None),
                    Instr::LocalGet(3),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::ArrayNew(T_ARRAY.into()),
                    Instr::LocalSet(4),
                    Instr::LocalGet(3),
                    Instr::I32Eqz,
                    Instr::If {
                        result: None,
                        then_body: vec![],
                        else_body: vec![
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::I32Const(0),
                            Instr::LocalGet(0),
                            Instr::StructGet(T_PVEC.into(), PV_TAIL),
                            Instr::I32Const(0),
                            Instr::LocalGet(3),
                            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                        ],
                    },
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(3),
                    Instr::LocalGet(1),
                    Instr::ArraySet(T_ARRAY.into()),
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalGet(0),
                    Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                    Instr::LocalGet(0),
                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_PVEC.into()),
                ],
                else_body: vec![
                    Instr::LocalGet(0),
                    Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                    Instr::LocalSet(6),
                    Instr::LocalGet(0),
                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                    Instr::RefIsNull,
                    Instr::If {
                        result: None,
                        then_body: vec![
                            Instr::I32Const(B),
                            Instr::LocalSet(6),
                            Instr::I32Const(B),
                            Instr::LocalGet(0),
                            Instr::StructGet(T_PVEC.into(), PV_TAIL),
                            Instr::RefCast {
                                nullable: false,
                                heap: HeapType::Eq,
                            },
                            Instr::Call("new_path".into()),
                            Instr::LocalSet(5),
                        ],
                        else_body: vec![
                            Instr::LocalGet(2),
                            Instr::I32Const(B),
                            Instr::I32ShrU,
                            Instr::I32Const(1),
                            Instr::LocalGet(6),
                            Instr::I32Shl,
                            Instr::I32GtU,
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    Instr::RefNull(HeapType::Eq),
                                    Instr::I32Const(BF),
                                    Instr::ArrayNew(T_VEC_CHILDREN.into()),
                                    Instr::LocalSet(7),
                                    Instr::LocalGet(7),
                                    Instr::RefAsNonNull,
                                    Instr::I32Const(0),
                                    Instr::LocalGet(0),
                                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                                    Instr::RefAsNonNull,
                                    Instr::RefCast {
                                        nullable: false,
                                        heap: HeapType::Eq,
                                    },
                                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                                    Instr::LocalGet(7),
                                    Instr::RefAsNonNull,
                                    Instr::I32Const(1),
                                    Instr::LocalGet(6),
                                    Instr::LocalGet(0),
                                    Instr::StructGet(T_PVEC.into(), PV_TAIL),
                                    Instr::RefCast {
                                        nullable: false,
                                        heap: HeapType::Eq,
                                    },
                                    Instr::Call("new_path".into()),
                                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                                    Instr::LocalGet(7),
                                    Instr::RefAsNonNull,
                                    Instr::RefNull(HeapType::Named(T_I32_ARRAY.into())),
                                    Instr::StructNew(T_VEC_INTERNAL.into()),
                                    Instr::RefCast {
                                        nullable: false,
                                        heap: HeapType::Eq,
                                    },
                                    Instr::LocalSet(5),
                                    Instr::LocalGet(6),
                                    Instr::I32Const(B),
                                    Instr::I32Add,
                                    Instr::LocalSet(6),
                                ],
                                else_body: vec![
                                    Instr::LocalGet(2),
                                    Instr::LocalGet(6),
                                    Instr::LocalGet(0),
                                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                                    Instr::LocalGet(0),
                                    Instr::StructGet(T_PVEC.into(), PV_TAIL),
                                    Instr::RefCast {
                                        nullable: false,
                                        heap: HeapType::Eq,
                                    },
                                    Instr::Call("push_tail".into()),
                                    Instr::LocalSet(5),
                                ],
                            },
                        ],
                    },
                    Instr::LocalGet(1),
                    Instr::ArrayNewFixed(T_ARRAY.into(), 1),
                    Instr::LocalSet(4),
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalGet(6),
                    Instr::LocalGet(5),
                    Instr::RefCast {
                        nullable: true,
                        heap: HeapType::Named(T_VEC_INTERNAL.into()),
                    },
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_PVEC.into()),
                ],
            },
        ],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Public API
//
// Convention: public functions called via prelude (get, set, len, concat,
// slice, builder_*) accept nullable PVec params because user code binds
// vectors as `ref null $PVec`. Internal helpers (push, push_tail, etc.)
// accept non-null params; callers insert ref.as_non_null.
// ═══════════════════════════════════════════════════════════════════════════

/// `make(len: i32, fill: anyref) -> PVec`
fn make_fn() -> FuncDef {
    // p0=len, p1=fill, L2=vec, L3=i
    FuncDef {
        name: "make".into(),
        params: vec![ValType::I32, ValType::Anyref],
        results: vec![ref_pvec()],
        locals: vec![ref_pvec_null(), ValType::I32],
        body: vec![
            // if len == 0: return empty_pvec
            Instr::LocalGet(0),
            Instr::I32Eqz,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: vec![Instr::GlobalGet("empty_pvec".into())],
                else_body: vec![
                    // vec = empty_pvec; i = 0; loop push
                    Instr::GlobalGet("empty_pvec".into()),
                    Instr::LocalSet(2),
                    Instr::I32Const(0),
                    Instr::LocalSet(3),
                    Instr::Block {
                        label: "brk".into(),
                        result: None,
                        body: vec![Instr::Loop {
                            label: "lp".into(),
                            result: None,
                            body: vec![
                                Instr::LocalGet(3),
                                Instr::LocalGet(0),
                                Instr::I32GeS,
                                Instr::BrIf("brk".into()),
                                Instr::LocalGet(2),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(1),
                                Instr::Call("push".into()),
                                Instr::LocalSet(2),
                                Instr::LocalGet(3),
                                Instr::I32Const(1),
                                Instr::I32Add,
                                Instr::LocalSet(3),
                                Instr::Br("lp".into()),
                            ],
                        }],
                    },
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                ],
            },
        ],
    }
}

/// `get(vec: PVec, idx: i32) -> anyref`
fn get_fn() -> FuncDef {
    // p0=vec, p1=idx
    // L2=len, L3=tailoff, L4=node(VecInternal), L5=level
    FuncDef {
        name: "get".into(),
        params: vec![ref_pvec_null(), ValType::I32],
        results: vec![ValType::Anyref],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ref_vec_internal_null(),
            ValType::I32,
        ],
        body: vec![
            // len = vec.len
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(2),
            // Small-vector fast path: len <= 32 => tail-only.
            Instr::LocalGet(2),
            Instr::I32Const(BF),
            Instr::I32LeS,
            Instr::If {
                result: Some(ValType::Anyref),
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_TAIL),
                    Instr::LocalGet(1),
                    Instr::I32Const(MASK),
                    Instr::I32And,
                    Instr::ArrayGet(T_ARRAY.into()),
                ],
                else_body: vec![
                    // tailoff = ((len - 1) >> 5) << 5
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::I32Const(B),
                    Instr::I32ShrU,
                    Instr::I32Const(B),
                    Instr::I32Shl,
                    Instr::LocalSet(3),
                    // Tail fast path
                    Instr::LocalGet(1),
                    Instr::LocalGet(3),
                    Instr::I32GeS,
                    Instr::If {
                        result: Some(ValType::Anyref),
                        then_body: vec![
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_PVEC.into(), PV_TAIL),
                            Instr::LocalGet(1),
                            Instr::I32Const(MASK),
                            Instr::I32And,
                            Instr::ArrayGet(T_ARRAY.into()),
                        ],
                        else_body: {
                            let b = vec![
                                // node = root
                                Instr::LocalGet(0),
                                Instr::RefAsNonNull,
                                Instr::StructGet(T_PVEC.into(), PV_ROOT),
                                Instr::LocalSet(4),
                                // level = shift
                                Instr::LocalGet(0),
                                Instr::RefAsNonNull,
                                Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                                Instr::LocalSet(5),
                                // While level > 5, descend through internal nodes.
                                Instr::Block {
                                    label: "brk".into(),
                                    result: None,
                                    body: vec![Instr::Loop {
                                        label: "lp".into(),
                                        result: None,
                                        body: vec![
                                            Instr::LocalGet(5),
                                            Instr::I32Const(B),
                                            Instr::I32LeS,
                                            Instr::BrIf("brk".into()),
                                            Instr::LocalGet(4),
                                            Instr::RefAsNonNull,
                                            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                                            Instr::LocalGet(1),
                                            Instr::LocalGet(5),
                                            Instr::I32ShrU,
                                            Instr::I32Const(MASK),
                                            Instr::I32And,
                                            Instr::ArrayGet(T_VEC_CHILDREN.into()),
                                            Instr::RefCast {
                                                nullable: true,
                                                heap: HeapType::Named(T_VEC_INTERNAL.into()),
                                            },
                                            Instr::LocalSet(4),
                                            Instr::LocalGet(5),
                                            Instr::I32Const(B),
                                            Instr::I32Sub,
                                            Instr::LocalSet(5),
                                            Instr::Br("lp".into()),
                                        ],
                                    }],
                                },
                                // Final child is a leaf; index directly and return the element.
                                Instr::LocalGet(4),
                                Instr::RefAsNonNull,
                                Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                                Instr::LocalGet(1),
                                Instr::LocalGet(5),
                                Instr::I32ShrU,
                                Instr::I32Const(MASK),
                                Instr::I32And,
                                Instr::ArrayGet(T_VEC_CHILDREN.into()),
                                Instr::RefCast {
                                    nullable: false,
                                    heap: HeapType::Named(T_ARRAY.into()),
                                },
                                Instr::LocalGet(1),
                                Instr::I32Const(MASK),
                                Instr::I32And,
                                Instr::ArrayGet(T_ARRAY.into()),
                            ];
                            b
                        },
                    },
                ],
            },
        ],
    }
}

/// `set(vec: PVec, idx: i32, val: anyref) -> PVec`
fn set_fn() -> FuncDef {
    // p0=vec, p1=idx, p2=val
    // L3=tailoff_val, L4=new_tail_data
    FuncDef {
        name: "set".into(),
        params: vec![ref_pvec_null(), ValType::I32, ValType::Anyref],
        results: vec![ref_pvec()],
        locals: vec![
            ValType::I32,     // L3: tailoff
            ref_array_null(), // L4: new_tail_data
        ],
        body: vec![
            // tailoff = tailoff(vec.len)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::Call("tailoff".into()),
            Instr::LocalSet(3),
            // if idx >= tailoff: update in tail
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::I32GeS,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: {
                    vec![
                        // Copy tail data
                        Instr::RefNull(HeapType::None),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        Instr::ArrayLen,
                        Instr::ArrayNew(T_ARRAY.into()),
                        Instr::LocalSet(4),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::I32Const(0),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        Instr::I32Const(0),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        Instr::ArrayLen,
                        Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                        // new_tail_data[idx - tailoff] = val
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(1),
                        Instr::LocalGet(3),
                        Instr::I32Sub,
                        Instr::LocalGet(2),
                        Instr::ArraySet(T_ARRAY.into()),
                        // PVec { len, shift, root, new_tail_data }
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_LEN),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructNew(T_PVEC.into()),
                    ]
                },
                else_body: {
                    vec![
                        // Update in trie via do_set
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_LEN),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        // new_root = do_set(shift, root as eqref, idx, val)
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                        Instr::RefAsNonNull,
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Eq,
                        },
                        Instr::LocalGet(1),
                        Instr::LocalGet(2),
                        Instr::Call("do_set".into()),
                        // Cast result back to VecInternal
                        Instr::RefCast {
                            nullable: true,
                            heap: HeapType::Named(T_VEC_INTERNAL.into()),
                        },
                        // tail stays the same
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        Instr::StructNew(T_PVEC.into()),
                    ]
                },
            },
        ],
    }
}

/// `len(vec: PVec) -> i32`
fn len_fn() -> FuncDef {
    FuncDef {
        name: "len".into(),
        params: vec![ref_pvec_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
        ],
    }
}

/// `concat(a: PVec, b: PVec) -> PVec`
/// v1: iterate b and push each element onto a.
fn concat_fn() -> FuncDef {
    // p0=a, p1=b, L2=result, L3=i, L4=b_len
    FuncDef {
        name: "concat".into(),
        params: vec![ref_pvec_null(), ref_pvec_null()],
        results: vec![ref_pvec()],
        locals: vec![ref_pvec_null(), ValType::I32, ValType::I32],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalSet(2),
            // b_len = b.len
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(4),
            // i = 0
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "brk".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "lp".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(4),
                        Instr::I32GeS,
                        Instr::BrIf("brk".into()),
                        // result = push(result, get(b, i))
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(1),
                        Instr::LocalGet(3),
                        Instr::Call("get".into()),
                        Instr::Call("push".into()),
                        Instr::LocalSet(2),
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("lp".into()),
                    ],
                }],
            },
            Instr::LocalGet(2),
            Instr::RefAsNonNull,
        ],
    }
}

/// `slice(vec: PVec, start: i32, end: i32) -> PVec`
/// v1: push elements [start, end) into a new empty vector.
fn slice_fn() -> FuncDef {
    // p0=vec, p1=start, p2=end, L3=result, L4=i
    FuncDef {
        name: "slice".into(),
        params: vec![ref_pvec_null(), ValType::I32, ValType::I32],
        results: vec![ref_pvec()],
        locals: vec![ref_pvec_null(), ValType::I32],
        body: vec![
            Instr::GlobalGet("empty_pvec".into()),
            Instr::LocalSet(3),
            Instr::LocalGet(1),
            Instr::LocalSet(4),
            Instr::Block {
                label: "brk".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "lp".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(4),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("brk".into()),
                        // result = push(result, get(vec, i))
                        Instr::LocalGet(3),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(0),
                        Instr::LocalGet(4),
                        Instr::Call("get".into()),
                        Instr::Call("push".into()),
                        Instr::LocalSet(3),
                        Instr::LocalGet(4),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(4),
                        Instr::Br("lp".into()),
                    ],
                }],
            },
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}

/// `pop_tail(len: i32, level: i32, node: VecInternal?) -> eqref`
/// Remove the rightmost leaf path from the trie. `len` is the OLD length; the
/// rightmost element lives at index len-2. Returns the new node, or null when
/// this subtree becomes empty. Inverse of push_tail.
fn pop_tail_fn() -> FuncDef {
    // p0=len, p1=level, p2=node
    // L3=subidx, L4=new_children, L5=newchild, L6=child
    FuncDef {
        name: "pop_tail".into(),
        params: vec![ValType::I32, ValType::I32, ref_vec_internal_null()],
        results: vec![ref_eq_null()],
        locals: vec![
            ValType::I32,
            ref_vec_children_null(),
            ref_eq_null(),
            ref_vec_internal_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::I32Const(2),
            Instr::I32Sub,
            Instr::LocalGet(1),
            Instr::I32ShrU,
            Instr::I32Const(MASK),
            Instr::I32And,
            Instr::LocalSet(3),
            Instr::LocalGet(1),
            Instr::I32Const(B),
            Instr::I32GtS,
            Instr::If {
                result: Some(ref_eq_null()),
                then_body: vec![
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                    Instr::LocalGet(3),
                    Instr::ArrayGet(T_VEC_CHILDREN.into()),
                    Instr::RefCast {
                        nullable: true,
                        heap: HeapType::Named(T_VEC_INTERNAL.into()),
                    },
                    Instr::LocalSet(6),
                    Instr::LocalGet(0),
                    Instr::LocalGet(1),
                    Instr::I32Const(B),
                    Instr::I32Sub,
                    Instr::LocalGet(6),
                    Instr::Call("pop_tail".into()),
                    Instr::LocalSet(5),
                    Instr::LocalGet(5),
                    Instr::RefIsNull,
                    Instr::LocalGet(3),
                    Instr::I32Eqz,
                    Instr::I32And,
                    Instr::If {
                        result: Some(ref_eq_null()),
                        then_body: vec![Instr::RefNull(HeapType::Eq)],
                        else_body: vec![
                            Instr::RefNull(HeapType::Eq),
                            Instr::I32Const(BF),
                            Instr::ArrayNew(T_VEC_CHILDREN.into()),
                            Instr::LocalSet(4),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::I32Const(0),
                            Instr::LocalGet(2),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                            Instr::I32Const(0),
                            Instr::I32Const(BF),
                            Instr::ArrayCopy(T_VEC_CHILDREN.into(), T_VEC_CHILDREN.into()),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::LocalGet(3),
                            Instr::LocalGet(5),
                            Instr::ArraySet(T_VEC_CHILDREN.into()),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::RefNull(HeapType::Named(T_I32_ARRAY.into())),
                            Instr::StructNew(T_VEC_INTERNAL.into()),
                            Instr::RefCast {
                                nullable: false,
                                heap: HeapType::Eq,
                            },
                        ],
                    },
                ],
                else_body: vec![
                    Instr::LocalGet(3),
                    Instr::I32Eqz,
                    Instr::If {
                        result: Some(ref_eq_null()),
                        then_body: vec![Instr::RefNull(HeapType::Eq)],
                        else_body: vec![
                            Instr::RefNull(HeapType::Eq),
                            Instr::I32Const(BF),
                            Instr::ArrayNew(T_VEC_CHILDREN.into()),
                            Instr::LocalSet(4),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::I32Const(0),
                            Instr::LocalGet(2),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                            Instr::I32Const(0),
                            Instr::I32Const(BF),
                            Instr::ArrayCopy(T_VEC_CHILDREN.into(), T_VEC_CHILDREN.into()),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::LocalGet(3),
                            Instr::RefNull(HeapType::Eq),
                            Instr::ArraySet(T_VEC_CHILDREN.into()),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::RefNull(HeapType::Named(T_I32_ARRAY.into())),
                            Instr::StructNew(T_VEC_INTERNAL.into()),
                            Instr::RefCast {
                                nullable: false,
                                heap: HeapType::Eq,
                            },
                        ],
                    },
                ],
            },
        ],
    }
}

/// `drop_last(vec: PVec?) -> PVec`
/// Return vec without its last element; the empty vector if already empty.
/// O(1) amortized: shrinks the tail, pulling the last trie leaf back into the
/// tail only at a 32-boundary (then O(log n)). Inverse of push.
fn drop_last_fn() -> FuncDef {
    // p0=vec
    // L1=len, L2=tailoff, L3=tail_len, L4=new_tail, L5=new_root, L6=new_shift, L7=shift
    FuncDef {
        name: "drop_last".into(),
        params: vec![ref_pvec_null()],
        results: vec![ref_pvec()],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ref_array_null(),
            ref_eq_null(),
            ValType::I32,
            ValType::I32,
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(1),
            Instr::LocalGet(1),
            Instr::I32Const(1),
            Instr::I32LeS,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: vec![Instr::GlobalGet("empty_pvec".into())],
                else_body: vec![
                    Instr::LocalGet(1),
                    Instr::Call("tailoff".into()),
                    Instr::LocalSet(2),
                    Instr::LocalGet(1),
                    Instr::LocalGet(2),
                    Instr::I32Sub,
                    Instr::LocalSet(3),
                    Instr::LocalGet(3),
                    Instr::I32Const(1),
                    Instr::I32GtS,
                    Instr::If {
                        result: Some(ref_pvec()),
                        then_body: vec![
                            // CASE A: tail_len > 1, shrink tail
                            Instr::RefNull(HeapType::None),
                            Instr::LocalGet(3),
                            Instr::I32Const(1),
                            Instr::I32Sub,
                            Instr::ArrayNew(T_ARRAY.into()),
                            Instr::LocalSet(4),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::I32Const(0),
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_PVEC.into(), PV_TAIL),
                            Instr::I32Const(0),
                            Instr::LocalGet(3),
                            Instr::I32Const(1),
                            Instr::I32Sub,
                            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                            Instr::LocalGet(1),
                            Instr::I32Const(1),
                            Instr::I32Sub,
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_PVEC.into(), PV_ROOT),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::StructNew(T_PVEC.into()),
                        ],
                        else_body: vec![
                            // CASE B: tail_len == 1, pull last leaf into a fresh tail
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                            Instr::LocalSet(7),
                            Instr::RefNull(HeapType::None),
                            Instr::I32Const(BF),
                            Instr::ArrayNew(T_ARRAY.into()),
                            Instr::LocalSet(4),
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::I32Const(0),
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::LocalGet(1),
                            Instr::I32Const(2),
                            Instr::I32Sub,
                            Instr::Call("get_leaf".into()),
                            Instr::I32Const(0),
                            Instr::I32Const(BF),
                            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                            Instr::LocalGet(1),
                            Instr::LocalGet(7),
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_PVEC.into(), PV_ROOT),
                            Instr::Call("pop_tail".into()),
                            Instr::LocalSet(5),
                            Instr::LocalGet(7),
                            Instr::LocalSet(6),
                            Instr::LocalGet(5),
                            Instr::RefIsNull,
                            Instr::If {
                                result: None,
                                then_body: vec![Instr::I32Const(0), Instr::LocalSet(6)],
                                else_body: vec![
                                    Instr::LocalGet(7),
                                    Instr::I32Const(B),
                                    Instr::I32GtS,
                                    Instr::If {
                                        result: None,
                                        then_body: vec![
                                            Instr::LocalGet(5),
                                            Instr::RefCast {
                                                nullable: false,
                                                heap: HeapType::Named(T_VEC_INTERNAL.into()),
                                            },
                                            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                                            Instr::I32Const(1),
                                            Instr::ArrayGet(T_VEC_CHILDREN.into()),
                                            Instr::RefIsNull,
                                            Instr::If {
                                                result: None,
                                                then_body: vec![
                                                    Instr::LocalGet(5),
                                                    Instr::RefCast {
                                                        nullable: false,
                                                        heap: HeapType::Named(
                                                            T_VEC_INTERNAL.into(),
                                                        ),
                                                    },
                                                    Instr::StructGet(
                                                        T_VEC_INTERNAL.into(),
                                                        VI_CHILDREN,
                                                    ),
                                                    Instr::I32Const(0),
                                                    Instr::ArrayGet(T_VEC_CHILDREN.into()),
                                                    Instr::LocalSet(5),
                                                    Instr::LocalGet(7),
                                                    Instr::I32Const(B),
                                                    Instr::I32Sub,
                                                    Instr::LocalSet(6),
                                                ],
                                                else_body: vec![],
                                            },
                                        ],
                                        else_body: vec![],
                                    },
                                ],
                            },
                            Instr::LocalGet(1),
                            Instr::I32Const(1),
                            Instr::I32Sub,
                            Instr::LocalGet(6),
                            Instr::LocalGet(5),
                            Instr::RefCast {
                                nullable: true,
                                heap: HeapType::Named(T_VEC_INTERNAL.into()),
                            },
                            Instr::LocalGet(4),
                            Instr::RefAsNonNull,
                            Instr::StructNew(T_PVEC.into()),
                        ],
                    },
                ],
            },
        ],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Builder: transient mutable-tail design
// ═══════════════════════════════════════════════════════════════════════════
//
// Builder layout (3-slot $Array, same ABI shape as before):
//   [0] = pvec_so_far : ref $PVec (trie prefix, tail always empty)
//   [1] = tail_len    : BoxedInt
//   [2] = tail_buf    : ref $Array (mutable 32-slot buffer)
//
// On push: write into tail_buf[tail_len] in place. When tail_buf fills (32),
// promote it as a VecLeaf into pvec_so_far, allocate fresh tail_buf.
// On freeze: construct final PVec from pvec_so_far's trie + tail_buf[0..tail_len].

/// `builder_new() -> Array`
fn builder_new_fn() -> FuncDef {
    FuncDef {
        name: "builder_new".into(),
        params: vec![],
        results: vec![ref_array()],
        locals: vec![],
        body: vec![
            // [0] = empty_pvec
            Instr::GlobalGet("empty_pvec".into()),
            // [1] = BoxedInt(0)
            Instr::I64Const(0),
            Instr::StructNew(T_BOXED_INT.into()),
            // [2] = new Array(32, null) — pre-allocated tail buffer
            Instr::RefNull(HeapType::None),
            Instr::I32Const(BF),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::ArrayNewFixed(T_ARRAY.into(), 3),
        ],
    }
}

/// `builder_from(vec: PVec) -> Array`
/// Seed builder from existing vector. Split at tail boundary.
fn builder_from_fn() -> FuncDef {
    // p0=vec
    // L1=tail_data, L2=tail_len, L3=new_tail_buf, L4=trie_prefix
    FuncDef {
        name: "builder_from".into(),
        params: vec![ref_pvec_null()],
        results: vec![ref_array()],
        locals: vec![
            ref_array_null(), // L1: tail_data
            ValType::I32,     // L2: tail_len
            ref_array_null(), // L3: new_tail_buf
            ref_pvec_null(),  // L4: trie_prefix (pvec with empty tail)
        ],
        body: vec![
            // tail_data = vec.tail
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_TAIL),
            Instr::LocalSet(1),
            // tail_len = tail_data.len
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            // new_tail_buf = new Array(32, null)
            Instr::RefNull(HeapType::None),
            Instr::I32Const(BF),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(3),
            // copy tail_data[0..tail_len] → new_tail_buf[0..tail_len]
            Instr::LocalGet(2),
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![],
                else_body: vec![
                    Instr::LocalGet(3),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(2),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                ],
            },
            // trie_prefix = PVec { len - tail_len, shift, root, empty_leaf }
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalGet(2),
            Instr::I32Sub,
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_SHIFT),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_ROOT),
            Instr::GlobalGet("empty_leaf".into()),
            Instr::StructNew(T_PVEC.into()),
            Instr::LocalSet(4),
            // return [trie_prefix, BoxedInt(tail_len), new_tail_buf]
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::LocalGet(2),
            Instr::I64ExtendI32S,
            Instr::StructNew(T_BOXED_INT.into()),
            Instr::LocalGet(3),
            Instr::ArrayNewFixed(T_ARRAY.into(), 3),
        ],
    }
}

/// `builder_push(builder: Array, elem: anyref) -> void`
fn builder_push_fn() -> FuncDef {
    // p0=builder, p1=elem
    // L2=tail_buf, L3=tail_len, L4=pvec_so_far, L5=full_leaf
    FuncDef {
        name: "builder_push".into(),
        params: vec![ref_array_null(), ValType::Anyref],
        results: vec![],
        locals: vec![
            ref_array_null(), // L2: tail_buf
            ValType::I32,     // L3: tail_len
            ref_pvec_null(),  // L4: pvec_so_far
            ref_pvec_null(),  // L5: new pvec after promote
        ],
        body: vec![
            // tail_buf = builder[2] as Array
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(2),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.into()),
            },
            Instr::LocalSet(2),
            // tail_len = unbox(builder[1])
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(1),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::StructGet(T_BOXED_INT.into(), 0),
            Instr::I32WrapI64,
            Instr::LocalSet(3),
            // if tail_len < 32: in-place append
            Instr::LocalGet(3),
            Instr::I32Const(BF),
            Instr::I32LtS,
            Instr::If {
                result: None,
                then_body: vec![
                    // tail_buf[tail_len] = elem
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(3),
                    Instr::LocalGet(1),
                    Instr::ArraySet(T_ARRAY.into()),
                    // builder[1] = BoxedInt(tail_len + 1)
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::I32Const(1),
                    Instr::LocalGet(3),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::I64ExtendI32S,
                    Instr::StructNew(T_BOXED_INT.into()),
                    Instr::ArraySet(T_ARRAY.into()),
                ],
                else_body: vec![
                    // Tail full: promote tail_buf as leaf into pvec_so_far
                    // pvec_so_far = builder[0] as PVec
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::ArrayGet(T_ARRAY.into()),
                    Instr::RefCast {
                        nullable: true,
                        heap: HeapType::Named(T_PVEC.into()),
                    },
                    Instr::LocalSet(4),
                    // We need to push the full tail_buf as a leaf into pvec_so_far.
                    // The simplest way: construct a temporary PVec that has tail_buf as its tail,
                    // then call push to trigger the promote logic.
                    // Actually, let's build the promoted pvec directly:
                    // Create a PVec with the full tail_buf as tail, then push to promote.
                    // temp_pvec = PVec { pvec_so_far.len + 32, pvec_so_far.shift, pvec_so_far.root, VecLeaf{tail_buf} }
                    // Then we need to "push" this tail into the trie.
                    //
                    // Simpler: use the push function with a temporary vector.
                    // Actually the cleanest approach: reconstruct pvec with the full tail,
                    // which is what the vector would look like if we'd been pushing one at a time.
                    // Then call push(that_vec, elem) which will detect tail is full and promote.
                    //
                    // temp = PVec { pvec_so_far.len + 32, shift, root, VecLeaf(tail_buf) }
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_LEN),
                    Instr::I32Const(BF),
                    Instr::I32Add,
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_PVEC.into()),
                    // push(temp, elem) — this promotes the full tail into the trie
                    Instr::LocalGet(1),
                    Instr::Call("push".into()),
                    Instr::LocalSet(5),
                    // Now split the result: trie goes to pvec_so_far, tail to new tail_buf
                    // The push result has a 1-element tail (the elem we just pushed).
                    // pvec_so_far = PVec { result.len - 1, result.shift, result.root, empty_leaf }
                    // Actually, we need to be more careful. After push, the result's trie
                    // contains all the full leaves, and result's tail has [elem].
                    // For the builder, pvec_so_far should have the trie part with empty tail.
                    // builder[0] = PVec { result.len - 1, result.shift, result.root, empty_leaf }
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    // new pvec_so_far
                    Instr::LocalGet(5),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_LEN),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::LocalGet(5),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                    Instr::LocalGet(5),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                    Instr::GlobalGet("empty_leaf".into()),
                    Instr::StructNew(T_PVEC.into()),
                    Instr::ArraySet(T_ARRAY.into()),
                    // Allocate fresh tail_buf, write elem at [0]
                    Instr::RefNull(HeapType::None),
                    Instr::I32Const(BF),
                    Instr::ArrayNew(T_ARRAY.into()),
                    Instr::LocalSet(2),
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::ArraySet(T_ARRAY.into()),
                    // builder[2] = new tail_buf
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::I32Const(2),
                    Instr::LocalGet(2),
                    Instr::ArraySet(T_ARRAY.into()),
                    // builder[1] = BoxedInt(1)
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::I32Const(1),
                    Instr::I64Const(1),
                    Instr::StructNew(T_BOXED_INT.into()),
                    Instr::ArraySet(T_ARRAY.into()),
                ],
            },
        ],
    }
}

/// `builder_extend(builder: Array, vec: PVec) -> void`
/// Iterate vec elements and push each into builder.
fn builder_extend_fn() -> FuncDef {
    // p0=builder, p1=vec
    // L2=i, L3=vec_len
    FuncDef {
        name: "builder_extend".into(),
        params: vec![ref_array_null(), ref_pvec_null()],
        results: vec![],
        locals: vec![ValType::I32, ValType::I32],
        body: vec![
            // vec_len = vec.len
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(3),
            // i = 0
            Instr::I32Const(0),
            Instr::LocalSet(2),
            Instr::Block {
                label: "brk".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "lp".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(2),
                        Instr::LocalGet(3),
                        Instr::I32GeS,
                        Instr::BrIf("brk".into()),
                        // builder_push(builder, get(vec, i))
                        Instr::LocalGet(0),
                        Instr::LocalGet(1),
                        Instr::LocalGet(2),
                        Instr::Call("get".into()),
                        Instr::Call("builder_push".into()),
                        Instr::LocalGet(2),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(2),
                        Instr::Br("lp".into()),
                    ],
                }],
            },
        ],
    }
}

/// `builder_freeze(builder: Array) -> PVec`
/// Construct final PVec from pvec_so_far's trie + tail_buf[0..tail_len].
fn builder_freeze_fn() -> FuncDef {
    // p0=builder
    // L1=pvec_so_far, L2=tail_len, L3=tail_buf, L4=final_tail_data
    FuncDef {
        name: "builder_freeze".into(),
        params: vec![ref_array_null()],
        results: vec![ref_pvec()],
        locals: vec![
            ref_pvec_null(),  // L1
            ValType::I32,     // L2
            ref_array_null(), // L3
            ref_array_null(), // L4
        ],
        body: vec![
            // pvec_so_far = builder[0] as PVec
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_PVEC.into()),
            },
            Instr::LocalSet(1),
            // tail_len = unbox(builder[1])
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(1),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::StructGet(T_BOXED_INT.into(), 0),
            Instr::I32WrapI64,
            Instr::LocalSet(2),
            // tail_buf = builder[2] as Array
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(2),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_ARRAY.into()),
            },
            Instr::LocalSet(3),
            // if tail_len == 0 and pvec_so_far.len == 0: return empty
            Instr::LocalGet(2),
            Instr::I32Eqz,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: vec![
                    // No tail elements. Return pvec_so_far (which has empty_leaf tail).
                    // If pvec_so_far.len == 0, this is the empty singleton.
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                ],
                else_body: vec![
                    // Build exact-sized tail: copy tail_buf[0..tail_len]
                    Instr::RefNull(HeapType::None),
                    Instr::LocalGet(2),
                    Instr::ArrayNew(T_ARRAY.into()),
                    Instr::LocalSet(4),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(3),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(2),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                    // return PVec { pvec_so_far.len + tail_len, shift, root, final_tail }
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_LEN),
                    Instr::LocalGet(2),
                    Instr::I32Add,
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                    Instr::LocalGet(1),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PVEC.into(), PV_ROOT),
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_PVEC.into()),
                ],
            },
        ],
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Array ↔ PVec boundary conversion
//
// Host functions (args, list_dir, env) return flat $Array. These helpers
// convert at the boundary.
// ═══════════════════════════════════════════════════════════════════════════

/// `from_array(arr: Array) -> PVec`
/// Wrap a flat $Array as a tail-only PVec (for ≤32 elements) or build via push.
fn from_array_fn() -> FuncDef {
    // p0=arr, L1=len, L2=result, L3=i
    FuncDef {
        name: "from_array".into(),
        params: vec![ref_array()],
        results: vec![ref_pvec()],
        locals: vec![ValType::I32, ref_pvec_null(), ValType::I32],
        body: vec![
            // len = arr.len
            Instr::LocalGet(0),
            Instr::ArrayLen,
            Instr::LocalSet(1),
            // if len == 0: return empty
            Instr::LocalGet(1),
            Instr::I32Eqz,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: vec![Instr::GlobalGet("empty_pvec".into())],
                else_body: vec![
                    // if len <= 32: tail-only PVec (zero-copy wrap)
                    Instr::LocalGet(1),
                    Instr::I32Const(BF),
                    Instr::I32LeS,
                    Instr::If {
                        result: Some(ref_pvec()),
                        then_body: vec![
                            Instr::LocalGet(1),
                            Instr::I32Const(0),
                            Instr::RefNull(HeapType::Named(T_VEC_INTERNAL.into())),
                            Instr::LocalGet(0),
                            Instr::StructNew(T_PVEC.into()),
                        ],
                        else_body: vec![
                            // len > 32: iterate and push
                            Instr::GlobalGet("empty_pvec".into()),
                            Instr::LocalSet(2),
                            Instr::I32Const(0),
                            Instr::LocalSet(3),
                            Instr::Block {
                                label: "brk".into(),
                                result: None,
                                body: vec![Instr::Loop {
                                    label: "lp".into(),
                                    result: None,
                                    body: vec![
                                        Instr::LocalGet(3),
                                        Instr::LocalGet(1),
                                        Instr::I32GeS,
                                        Instr::BrIf("brk".into()),
                                        Instr::LocalGet(2),
                                        Instr::RefAsNonNull,
                                        Instr::LocalGet(0),
                                        Instr::LocalGet(3),
                                        Instr::ArrayGet(T_ARRAY.into()),
                                        Instr::Call("push".into()),
                                        Instr::LocalSet(2),
                                        Instr::LocalGet(3),
                                        Instr::I32Const(1),
                                        Instr::I32Add,
                                        Instr::LocalSet(3),
                                        Instr::Br("lp".into()),
                                    ],
                                }],
                            },
                            Instr::LocalGet(2),
                            Instr::RefAsNonNull,
                        ],
                    },
                ],
            },
        ],
    }
}

/// `to_array(vec: PVec) -> Array`
/// Flatten a PVec back to a flat $Array (for host boundary, e.g. write_bytes).
fn to_array_fn() -> FuncDef {
    // p0=vec, L1=len, L2=result, L3=i
    FuncDef {
        name: "to_array".into(),
        params: vec![ref_pvec_null()],
        results: vec![ref_array()],
        locals: vec![ValType::I32, ref_array_null(), ValType::I32],
        body: vec![
            // len = vec.len
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(1),
            // result = new Array(len, null)
            Instr::RefNull(HeapType::None),
            Instr::LocalGet(1),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(2),
            // i = 0; loop: result[i] = get(vec, i)
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "brk".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "lp".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(1),
                        Instr::I32GeS,
                        Instr::BrIf("brk".into()),
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::Call("get".into()),
                        Instr::ArraySet(T_ARRAY.into()),
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("lp".into()),
                    ],
                }],
            },
            Instr::LocalGet(2),
            Instr::RefAsNonNull,
        ],
    }
}

/// `from_read_file_result(v: Variant) -> Variant`
/// Host read_file returns Result<Array, String>; rewrite Ok(Array) payload to Ok(PVec).
fn from_read_file_result_fn() -> FuncDef {
    let variant_ref = ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_VARIANT.into()),
    };

    FuncDef {
        name: "from_read_file_result".into(),
        params: vec![variant_ref.clone()],
        results: vec![variant_ref.clone()],
        locals: vec![variant_ref.clone(), ref_array_null()],
        body: vec![
            // If not the expected Result type / Ok arm, pass through unchanged.
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_VARIANT.into(), 0),
            Instr::I32Const(1),
            Instr::I32Eq,
            Instr::If {
                result: Some(variant_ref.clone()),
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_VARIANT.into(), 1),
                    Instr::I32Eqz,
                    Instr::If {
                        result: Some(variant_ref.clone()),
                        then_body: vec![
                            // payload = [from_array(payload[0])]
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_VARIANT.into(), 2),
                            Instr::LocalSet(2),
                            Instr::I32Const(1),
                            Instr::I32Const(0),
                            Instr::LocalGet(2),
                            Instr::RefAsNonNull,
                            Instr::I32Const(0),
                            Instr::ArrayGet(T_ARRAY.into()),
                            Instr::RefCast {
                                nullable: false,
                                heap: HeapType::Named(T_ARRAY.into()),
                            },
                            Instr::Call("from_array".into()),
                            Instr::ArrayNewFixed(T_ARRAY.into(), 1),
                            Instr::StructNew(T_VARIANT.into()),
                        ],
                        else_body: vec![Instr::LocalGet(0)],
                    },
                ],
                else_body: vec![Instr::LocalGet(0)],
            },
        ],
    }
}
