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

/// $PVec field indices
const PV_LEN: u32 = 0;
const PV_SHIFT: u32 = 1;
const PV_ROOT: u32 = 2;
const PV_TAIL: u32 = 3;

/// $VecLeaf field index
const VL_DATA: u32 = 0;

/// $VecInternal field index
const VI_CHILDREN: u32 = 0;

/// Build the `rt.arr` module: persistent bit-partitioned trie vector operations.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.arr");

    // ── globals: empty vector singleton ──
    m.globals.push(GlobalDef {
        name: "empty_leaf".into(),
        mutable: false,
        ty: ref_vec_leaf(),
        init: vec![
            Instr::ArrayNewFixed(T_ARRAY.into(), 0),
            Instr::StructNew(T_VEC_LEAF.into()),
        ],
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
    // p0=vec, p1=idx, L2=node(VecNode), L3=level
    FuncDef {
        name: "get_leaf".into(),
        params: vec![ref_pvec(), ValType::I32],
        results: vec![ref_array()],
        locals: vec![ref_vec_node_null(), ValType::I32],
        body: vec![
            // if idx >= tailoff(len): return tail.data
            Instr::LocalGet(1),
            Instr::LocalGet(0),
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::Call("tailoff".into()),
            Instr::I32GeS,
            Instr::If {
                result: Some(ref_array()),
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::StructGet(T_PVEC.into(), PV_TAIL),
                    Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
                ],
                else_body: {
                    let mut b = vec![
                        // node = root (as VecNode)
                        Instr::LocalGet(0),
                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_VEC_NODE.into()),
                        },
                        Instr::LocalSet(2),
                        // level = shift
                        Instr::LocalGet(0),
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        Instr::LocalSet(3),
                    ];
                    // loop: while level > 0, descend
                    b.push(Instr::Block {
                        label: "brk".into(),
                        result: None,
                        body: vec![Instr::Loop {
                            label: "lp".into(),
                            result: None,
                            body: vec![
                                Instr::LocalGet(3),
                                Instr::I32Eqz,
                                Instr::BrIf("brk".into()),
                                // node = cast_internal(node).children[(idx >> level) & MASK]
                                Instr::LocalGet(2),
                                Instr::RefAsNonNull,
                                Instr::RefCast {
                                    nullable: false,
                                    heap: HeapType::Named(T_VEC_INTERNAL.into()),
                                },
                                Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                                Instr::LocalGet(1),
                                Instr::LocalGet(3),
                                Instr::I32ShrU,
                                Instr::I32Const(MASK),
                                Instr::I32And,
                                Instr::ArrayGet(T_VEC_CHILDREN.into()),
                                Instr::LocalSet(2),
                                // level -= B
                                Instr::LocalGet(3),
                                Instr::I32Const(B),
                                Instr::I32Sub,
                                Instr::LocalSet(3),
                                Instr::Br("lp".into()),
                            ],
                        }],
                    });
                    // node is a VecLeaf now
                    b.push(Instr::LocalGet(2));
                    b.push(Instr::RefAsNonNull);
                    b.push(Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_VEC_LEAF.into()),
                    });
                    b.push(Instr::StructGet(T_VEC_LEAF.into(), VL_DATA));
                    b
                },
            },
        ],
    }
}

/// `new_path(level: i32, node: VecNode) -> VecNode`
/// Wrap node in a chain of VecInternal nodes from level down to 0.
fn new_path_fn() -> FuncDef {
    // p0=level, p1=node, L2=children(tmp)
    FuncDef {
        name: "new_path".into(),
        params: vec![ValType::I32, ref_vec_node()],
        results: vec![ref_vec_node()],
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
                        // children = new VecChildren(32, null)
                        Instr::RefNull(HeapType::Named(T_VEC_NODE.into())),
                        Instr::I32Const(BF),
                        Instr::ArrayNew(T_VEC_CHILDREN.into()),
                        Instr::LocalSet(2),
                        // children[0] = node
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::I32Const(0),
                        Instr::LocalGet(1),
                        Instr::ArraySet(T_VEC_CHILDREN.into()),
                        // node = VecInternal { children } as VecNode
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::StructNew(T_VEC_INTERNAL.into()),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_VEC_NODE.into()),
                        },
                        Instr::LocalSet(1),
                        // level -= B
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

/// `push_tail(level: i32, parent: VecInternal, tail_node: VecNode) -> VecInternal`
/// Path-copy the rightmost spine of `parent` and insert `tail_node` at the bottom.
fn push_tail_fn() -> FuncDef {
    // push_tail(cnt: i32, level: i32, parent: VecInternal, tail_node: VecNode) -> VecNode
    //
    // Path-copy the rightmost spine of `parent` and insert `tail_node` at
    // the bottom. Called when the current tail is full and needs to be
    // promoted into the trie.
    //
    // Algorithm:
    //   sub_idx = ((cnt - 1) >> level) & MASK
    //   copy parent.children → new_children
    //   if level == B (bottom of internal nodes):
    //     new_children[sub_idx] = tail_node
    //   else:
    //     if parent.children[sub_idx] != null:
    //       new_children[sub_idx] = push_tail(cnt, level-B, children[sub_idx], tail_node)
    //     else:
    //       new_children[sub_idx] = new_path(level-B, tail_node)
    //   return VecInternal { new_children }
    //
    // p0=cnt, p1=level, p2=parent, p3=tail_node
    // L4=new_children, L5=sub_idx, L6=child
    FuncDef {
        name: "push_tail".into(),
        params: vec![
            ValType::I32,
            ValType::I32,
            ref_vec_internal_null(),
            ref_vec_node(),
        ],
        results: vec![ref_vec_node()],
        locals: vec![
            ref_vec_children_null(), // L4: new_children
            ValType::I32,            // L5: sub_idx
            ref_vec_node_null(),     // L6: child
        ],
        body: vec![
            // sub_idx = ((cnt - 1) >> level) & MASK
            Instr::LocalGet(0),
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::LocalGet(1),
            Instr::I32ShrU,
            Instr::I32Const(MASK),
            Instr::I32And,
            Instr::LocalSet(5),
            // copy parent.children → new_children
            Instr::RefNull(HeapType::Named(T_VEC_NODE.into())),
            Instr::I32Const(BF),
            Instr::ArrayNew(T_VEC_CHILDREN.into()),
            Instr::LocalSet(4),
            // array.copy new_children[0..32] <- parent.children[0..32]
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(2),
            Instr::RefAsNonNull,
            Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
            Instr::I32Const(0),
            Instr::I32Const(BF),
            Instr::ArrayCopy(T_VEC_CHILDREN.into(), T_VEC_CHILDREN.into()),
            // if level == B: insert tail_node directly
            Instr::LocalGet(1),
            Instr::I32Const(B),
            Instr::I32Eq,
            Instr::If {
                result: None,
                then_body: vec![
                    // new_children[sub_idx] = tail_node
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(5),
                    Instr::LocalGet(3),
                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                ],
                else_body: vec![
                    // child = parent.children[sub_idx]
                    Instr::LocalGet(2),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_VEC_INTERNAL.into(), VI_CHILDREN),
                    Instr::LocalGet(5),
                    Instr::ArrayGet(T_VEC_CHILDREN.into()),
                    Instr::LocalSet(6),
                    // if child != null: recurse; else: new_path
                    Instr::LocalGet(6),
                    Instr::RefIsNull,
                    Instr::If {
                        result: Some(ref_vec_node()),
                        then_body: vec![
                            // new_path(level - B, tail_node)
                            Instr::LocalGet(1),
                            Instr::I32Const(B),
                            Instr::I32Sub,
                            Instr::LocalGet(3),
                            Instr::Call("new_path".into()),
                        ],
                        else_body: vec![
                            // push_tail(cnt, level - B, child as VecInternal, tail_node)
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
                    // new_children[sub_idx] = child (result of recurse)
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(5),
                    Instr::LocalGet(6),
                    Instr::ArraySet(T_VEC_CHILDREN.into()),
                ],
            },
            // return VecInternal { new_children } as VecNode
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::StructNew(T_VEC_INTERNAL.into()),
            Instr::RefCast {
                nullable: false,
                heap: HeapType::Named(T_VEC_NODE.into()),
            },
        ],
    }
}

/// `do_set(level: i32, node: VecNode, idx: i32, val: anyref) -> VecNode`
/// Path-copy from current level down to the leaf, setting val at idx.
fn do_set_fn() -> FuncDef {
    // p0=level, p1=node, p2=idx, p3=val
    // L4=new_children, L5=sub_idx, L6=new_data, L7=leaf_data
    FuncDef {
        name: "do_set".into(),
        params: vec![ValType::I32, ref_vec_node(), ValType::I32, ValType::Anyref],
        results: vec![ref_vec_node()],
        locals: vec![
            ref_vec_children_null(), // L4
            ValType::I32,            // L5: sub_idx
            ref_array_null(),        // L6: new_data
            ref_array_null(),        // L7: leaf_data
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::I32Eqz,
            Instr::If {
                result: Some(ref_vec_node()),
                then_body: {
                    // level == 0: this is a leaf node
                    let mut b = vec![
                        // leaf_data = cast_leaf(node).data
                        Instr::LocalGet(1),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_VEC_LEAF.into()),
                        },
                        Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
                        Instr::LocalSet(7),
                        // new_data = new Array(leaf_data.len, null)
                        Instr::RefNull(HeapType::None),
                        Instr::LocalGet(7),
                        Instr::RefAsNonNull,
                        Instr::ArrayLen,
                        Instr::ArrayNew(T_ARRAY.into()),
                        Instr::LocalSet(6),
                        // copy leaf_data → new_data
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
                        // new_data[idx & MASK] = val
                        Instr::LocalGet(6),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(2),
                        Instr::I32Const(MASK),
                        Instr::I32And,
                        Instr::LocalGet(3),
                        Instr::ArraySet(T_ARRAY.into()),
                        // return VecLeaf { new_data } as VecNode
                        Instr::LocalGet(6),
                        Instr::RefAsNonNull,
                        Instr::StructNew(T_VEC_LEAF.into()),
                    ];
                    b.push(Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_VEC_NODE.into()),
                    });
                    b
                },
                else_body: {
                    // level > 0: internal node
                    vec![
                        // sub_idx = (idx >> level) & MASK
                        Instr::LocalGet(2),
                        Instr::LocalGet(0),
                        Instr::I32ShrU,
                        Instr::I32Const(MASK),
                        Instr::I32And,
                        Instr::LocalSet(5),
                        // copy children
                        Instr::RefNull(HeapType::Named(T_VEC_NODE.into())),
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
                        // new_children[sub_idx] = do_set(level-B, children[sub_idx], idx, val)
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(5),
                        // call do_set
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
                        Instr::RefAsNonNull, // child must be non-null for valid idx
                        Instr::LocalGet(2),
                        Instr::LocalGet(3),
                        Instr::Call("do_set".into()),
                        Instr::ArraySet(T_VEC_CHILDREN.into()),
                        // return VecInternal { new_children } as VecNode
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructNew(T_VEC_INTERNAL.into()),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_VEC_NODE.into()),
                        },
                    ]
                },
            },
        ],
    }
}

/// `push(vec: PVec, val: anyref) -> PVec`
/// Append a single element to the vector.
fn push_fn() -> FuncDef {
    // p0=vec, p1=val
    // L2=len, L3=tail_len, L4=new_tail_data, L5=new_root, L6=new_shift, L7=children(overflow)
    FuncDef {
        name: "push".into(),
        params: vec![ref_pvec(), ValType::Anyref],
        results: vec![ref_pvec()],
        locals: vec![
            ValType::I32,            // L2: len
            ValType::I32,            // L3: tail_len
            ref_array_null(),        // L4: new_tail_data
            ref_vec_node_null(),     // L5: new_root (as VecNode for intermediate)
            ValType::I32,            // L6: new_shift
            ref_vec_children_null(), // L7: children array (for overflow)
        ],
        body: vec![
            // len = vec.len
            Instr::LocalGet(0),
            Instr::StructGet(T_PVEC.into(), PV_LEN),
            Instr::LocalSet(2),
            // tail_len = vec.tail.data.len
            Instr::LocalGet(0),
            Instr::StructGet(T_PVEC.into(), PV_TAIL),
            Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
            Instr::ArrayLen,
            Instr::LocalSet(3),
            // if tail has room (tail_len < 32):
            Instr::LocalGet(3),
            Instr::I32Const(BF),
            Instr::I32LtS,
            Instr::If {
                result: Some(ref_pvec()),
                then_body: {
                    // Copy tail data and append val
                    vec![
                        // new_tail_data = new Array(tail_len + 1, null)
                        Instr::RefNull(HeapType::None),
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::ArrayNew(T_ARRAY.into()),
                        Instr::LocalSet(4),
                        // copy old tail data
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
                                Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
                                Instr::I32Const(0),
                                Instr::LocalGet(3),
                                Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                            ],
                        },
                        // new_tail_data[tail_len] = val
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::LocalGet(1),
                        Instr::ArraySet(T_ARRAY.into()),
                        // return PVec { len+1, shift, root, VecLeaf{new_tail_data} }
                        Instr::LocalGet(2),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalGet(0),
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        Instr::LocalGet(0),
                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructNew(T_VEC_LEAF.into()),
                        Instr::StructNew(T_PVEC.into()),
                    ]
                },
                else_body: {
                    // Tail is full — promote old tail into trie
                    vec![
                        // Wrap old tail as VecNode
                        // tail_node = VecLeaf(old_tail.data) as VecNode — already is a VecLeaf
                        // new_shift = vec.shift
                        Instr::LocalGet(0),
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        Instr::LocalSet(6),
                        // Check if root is null (tail-only vector that just filled up)
                        Instr::LocalGet(0),
                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                        Instr::RefIsNull,
                        Instr::If {
                            result: None,
                            then_body: vec![
                                // No root yet: create root with old tail as child
                                // new_shift = B
                                Instr::I32Const(B),
                                Instr::LocalSet(6),
                                // new_root = new_path(B, old_tail as VecNode)
                                // new_path wraps the leaf in one level of VecInternal
                                Instr::I32Const(B),
                                Instr::LocalGet(0),
                                Instr::StructGet(T_PVEC.into(), PV_TAIL),
                                Instr::RefCast {
                                    nullable: false,
                                    heap: HeapType::Named(T_VEC_NODE.into()),
                                },
                                Instr::Call("new_path".into()),
                                Instr::LocalSet(5),
                            ],
                            else_body: vec![
                                // Root exists: check if trie is full at current depth
                                // overflow? = (len >> B) > (1 << shift)
                                // Simpler: overflow if ((len - 1) >> B) >= (1 << shift)
                                // Actually: overflow when cnt >> B > 1 << shift
                                // cnt = len (before adding new element, which has len = old_len)
                                // The trie can hold (1 << (shift + B)) elements
                                // Trie is full when trie_count == (1 << (shift + B))
                                // trie_count = tailoff(len) = len - 32 (since tail is full, tail_len==32)
                                // Equivalently: (len >> B) > (1 << shift)
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
                                        // Overflow: need new root level
                                        // new_root_children = [old_root, new_path(shift, old_tail)]
                                        Instr::RefNull(HeapType::Named(T_VEC_NODE.into())),
                                        Instr::I32Const(BF),
                                        Instr::ArrayNew(T_VEC_CHILDREN.into()),
                                        Instr::LocalSet(7),
                                        // children[0] = old root as VecNode
                                        Instr::LocalGet(7),
                                        Instr::RefAsNonNull,
                                        Instr::I32Const(0),
                                        Instr::LocalGet(0),
                                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                                        Instr::RefAsNonNull,
                                        Instr::RefCast {
                                            nullable: false,
                                            heap: HeapType::Named(T_VEC_NODE.into()),
                                        },
                                        Instr::ArraySet(T_VEC_CHILDREN.into()),
                                        // children[1] = new_path(shift, old_tail as VecNode)
                                        Instr::LocalGet(7),
                                        Instr::RefAsNonNull,
                                        Instr::I32Const(1),
                                        Instr::LocalGet(6),
                                        Instr::LocalGet(0),
                                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                                        Instr::RefCast {
                                            nullable: false,
                                            heap: HeapType::Named(T_VEC_NODE.into()),
                                        },
                                        Instr::Call("new_path".into()),
                                        Instr::ArraySet(T_VEC_CHILDREN.into()),
                                        // new_root = VecInternal { children } as VecNode
                                        Instr::LocalGet(7),
                                        Instr::RefAsNonNull,
                                        Instr::StructNew(T_VEC_INTERNAL.into()),
                                        Instr::RefCast {
                                            nullable: false,
                                            heap: HeapType::Named(T_VEC_NODE.into()),
                                        },
                                        Instr::LocalSet(5),
                                        // new_shift += B
                                        Instr::LocalGet(6),
                                        Instr::I32Const(B),
                                        Instr::I32Add,
                                        Instr::LocalSet(6),
                                    ],
                                    else_body: vec![
                                        // Room in trie: push_tail
                                        Instr::LocalGet(2),
                                        Instr::LocalGet(6),
                                        Instr::LocalGet(0),
                                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                                        Instr::LocalGet(0),
                                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                                        Instr::RefCast {
                                            nullable: false,
                                            heap: HeapType::Named(T_VEC_NODE.into()),
                                        },
                                        Instr::Call("push_tail".into()),
                                        Instr::LocalSet(5),
                                    ],
                                },
                            ],
                        },
                        // Create new single-element tail
                        Instr::LocalGet(1),
                        Instr::ArrayNewFixed(T_ARRAY.into(), 1),
                        Instr::LocalSet(4),
                        // return PVec { len+1, new_shift, new_root as VecInternal, VecLeaf{new_tail} }
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
                        Instr::StructNew(T_VEC_LEAF.into()),
                        Instr::StructNew(T_PVEC.into()),
                    ]
                },
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
    FuncDef {
        name: "get".into(),
        params: vec![ref_pvec_null(), ValType::I32],
        results: vec![ValType::Anyref],
        locals: vec![],
        body: vec![
            // leaf = get_leaf(vec, idx)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::Call("get_leaf".into()),
            // leaf[idx & MASK]
            // For trie leaves: idx & MASK gives the position within the 32-element leaf.
            // For tail: tailoff is 32-aligned, so (idx - tailoff) == idx & MASK.
            Instr::LocalGet(1),
            Instr::I32Const(MASK),
            Instr::I32And,
            Instr::ArrayGet(T_ARRAY.into()),
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
                        Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
                        Instr::ArrayLen,
                        Instr::ArrayNew(T_ARRAY.into()),
                        Instr::LocalSet(4),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::I32Const(0),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
                        Instr::I32Const(0),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_TAIL),
                        Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
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
                        // PVec { len, shift, root, VecLeaf{new_tail_data} }
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
                        Instr::StructNew(T_VEC_LEAF.into()),
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
                        // new_root = do_set(shift, root as VecNode, idx, val)
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_SHIFT),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_PVEC.into(), PV_ROOT),
                        Instr::RefAsNonNull,
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_VEC_NODE.into()),
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
            // tail_data = vec.tail.data
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PVEC.into(), PV_TAIL),
            Instr::StructGet(T_VEC_LEAF.into(), VL_DATA),
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
                    Instr::StructNew(T_VEC_LEAF.into()),
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
                    // return PVec { pvec_so_far.len + tail_len, shift, root, VecLeaf{final_tail} }
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
                    Instr::StructNew(T_VEC_LEAF.into()),
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
                            Instr::StructNew(T_VEC_LEAF.into()),
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
