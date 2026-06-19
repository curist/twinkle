use crate::wasm::ir::*;

/// Qualified type names as they appear *after* linking (namespace "rt.types" → prefix "rt_types").
/// All other runtime modules use these constants so their instruction operands are already
/// correctly qualified and do not need linker rewriting.
pub const T_ARRAY: &str = "rt_types__Array";
pub const T_STRING: &str = "rt_types__String";
pub const T_STR_BUILDER: &str = "rt_types__StrBuilder";
pub const T_HAMT_ENTRY: &str = "rt_types__HamtEntry";
pub const T_HAMT_NODE: &str = "rt_types__HamtNode";
pub const T_HAMT_COLLISION: &str = "rt_types__HamtCollision";
pub const T_PDICT: &str = "rt_types__PDict";
pub const T_CLOSURE_ENV: &str = "rt_types__ClosureEnv";
pub const T_CLOSURE_FUNC: &str = "rt_types__ClosureFunc";
pub const T_CLOSURE: &str = "rt_types__Closure";
pub const T_VARIANT: &str = "rt_types__Variant";
pub const T_BOXED_INT: &str = "rt_types__BoxedInt";
pub const T_BOXED_FLOAT: &str = "rt_types__BoxedFloat";
pub const T_ITER_STATE: &str = "rt_types__IterState";
pub const T_TASK: &str = "rt_types__Task";
pub const T_VEC_CHILDREN: &str = "rt_types__VecChildren";
pub const T_VEC_INTERNAL: &str = "rt_types__VecInternal";
pub const T_I32_ARRAY: &str = "rt_types__I32Array";
pub const T_ARRAY_I64: &str = "rt_types__ArrayI64";
pub const T_ARRAY_F64: &str = "rt_types__ArrayF64";
pub const T_PVEC: &str = "rt_types__PVec";

/// Qualified function names for cross-module calls (after linking).
pub const F_STR_EQ: &str = "rt_str__eq";

/// Ref-to-Array (non-null)
pub fn ref_array() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_ARRAY.into()),
    }
}
/// Ref-to-Array (nullable)
pub fn ref_array_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_ARRAY.into()),
    }
}
/// Ref-to-String (non-null)
pub fn ref_string() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_STRING.into()),
    }
}
/// Ref-to-String (nullable)
pub fn ref_string_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_STRING.into()),
    }
}
/// Ref-to-StrBuilder (non-null)
pub fn ref_str_builder() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_STR_BUILDER.into()),
    }
}
/// Ref-to-StrBuilder (nullable)
pub fn ref_str_builder_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_STR_BUILDER.into()),
    }
}
/// Ref-to-PDict (non-null)
pub fn ref_pdict() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_PDICT.into()),
    }
}
/// Ref-to-PDict (nullable)
pub fn ref_pdict_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_PDICT.into()),
    }
}

/// Ref-to-PVec (non-null)
pub fn ref_pvec() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_PVEC.into()),
    }
}
/// Ref-to-PVec (nullable)
pub fn ref_pvec_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_PVEC.into()),
    }
}

/// Ref-to-VecChildren (non-null)
pub fn ref_vec_children() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_VEC_CHILDREN.into()),
    }
}
/// Ref-to-VecChildren (nullable)
#[allow(dead_code)]
pub fn ref_vec_children_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_VEC_CHILDREN.into()),
    }
}

/// Ref-to-VecInternal (non-null)
#[allow(dead_code)]
pub fn ref_vec_internal() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_VEC_INTERNAL.into()),
    }
}
/// Ref-to-VecInternal (nullable)
pub fn ref_vec_internal_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_VEC_INTERNAL.into()),
    }
}

/// Ref-to-I32Array (nullable) — RRB relaxed-node size table.
pub fn ref_i32_array_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_I32_ARRAY.into()),
    }
}

/// Ref-to-ArrayI64 (nullable) — dense i64 buffer for the native value sort.
pub fn ref_array_i64_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_ARRAY_I64.into()),
    }
}

/// Ref-to-ArrayF64 (nullable) — dense f64 buffer for the native value sort.
pub fn ref_array_f64_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_ARRAY_F64.into()),
    }
}

/// Ref-to-IterState (non-null)
pub fn ref_iter_state() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_ITER_STATE.into()),
    }
}
/// Ref-to-IterState (nullable)
pub fn ref_iter_state_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_ITER_STATE.into()),
    }
}

/// Build the `rt.types` module: all shared Wasm GC type definitions.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.types");

    // (type $Array (array (mut anyref)))
    m.types.push(TypeDef::Array {
        name: "Array".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::Anyref,
        },
    });

    // (type $String (array (mut i8)))
    // Mutable so we can write during construction; "immutable by convention" at the API level.
    m.types.push(TypeDef::Array {
        name: "String".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::I8,
        },
    });

    // (type $StrBuilder (struct (field $len (mut i32)) (field $buf (mut (ref $String)))))
    // Transient growable byte buffer for the string-builder optimization. Internal
    // only; never escapes to user code. Must come after $String (it refs it).
    m.types.push(TypeDef::Struct {
        name: "StrBuilder".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef {
                name: Some("len".into()),
                mutable: true,
                ty: ValType::I32,
            },
            FieldDef {
                name: Some("buf".into()),
                mutable: true,
                ty: ValType::Ref {
                    nullable: false,
                    heap: HeapType::Named("String".into()),
                },
            },
        ],
    });

    // -- Persistent vector trie types --
    //
    // Layout: VecChildren stores eqref (either VecInternal or Array directly).
    // No VecNode base or VecLeaf wrapper — leaf arrays live directly in VecChildren.
    // PVec.tail is a bare Array ref.
    // VecChildren/VecInternal/PVec must come before PDict (which has a ref $PVec field)
    // to avoid forward type references that V8's validator rejects.

    // (type $VecChildren (array (mut (ref null eq))))
    m.types.push(TypeDef::Array {
        name: "VecChildren".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::Ref {
                nullable: true,
                heap: HeapType::Eq,
            },
        },
    });

    // (type $I32Array (array (mut i32)))
    // RRB relaxed-node size table: cumulative element counts per child.
    // Must precede VecInternal, which carries a nullable ref to it.
    m.types.push(TypeDef::Array {
        name: "I32Array".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::I32,
        },
    });

    // (type $ArrayI64 (array (mut i64)))
    // Dense i64 scratch buffer for the native Vector<Int> value sort.
    m.types.push(TypeDef::Array {
        name: "ArrayI64".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::I64,
        },
    });

    // (type $ArrayF64 (array (mut f64)))
    // Dense f64 scratch buffer for the native Vector<Float> value sort.
    m.types.push(TypeDef::Array {
        name: "ArrayF64".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::F64,
        },
    });

    // (type $VecInternal (struct (field $children (ref $VecChildren))
    //                            (field $sizes (ref null $I32Array))))
    // sizes == null ⇒ regular (radix-indexed) node; non-null ⇒ relaxed (RRB) node.
    m.types.push(TypeDef::Struct {
        name: "VecInternal".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef {
                name: Some("children".into()),
                mutable: false,
                ty: ref_vec_children(),
            },
            FieldDef {
                name: Some("sizes".into()),
                mutable: false,
                ty: ref_i32_array_null(),
            },
        ],
    });

    // (type $PVec (struct (field $len i32) (field $shift i32)
    //                     (field $root (ref null $VecInternal)) (field $tail (ref $Array))))
    m.types.push(TypeDef::Struct {
        name: "PVec".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef::named("len", ValType::I32),
            FieldDef::named("shift", ValType::I32),
            FieldDef {
                name: Some("root".into()),
                mutable: false,
                ty: ref_vec_internal_null(),
            },
            FieldDef {
                name: Some("tail".into()),
                mutable: false,
                ty: ref_array(),
            },
        ],
    });

    // (type $HamtEntry (struct (field $hash i64) (field $key anyref) (field $val anyref)))
    m.types.push(TypeDef::Struct {
        name: "HamtEntry".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef::named("hash", ValType::I64),
            FieldDef::named("key", ValType::Anyref),
            FieldDef::named("val", ValType::Anyref),
        ],
    });

    // (type $HamtNode (struct (field $bitmap i32) (field $entries (ref $Array))))
    m.types.push(TypeDef::Struct {
        name: "HamtNode".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef::named("bitmap", ValType::I32),
            FieldDef {
                name: Some("entries".into()),
                mutable: false,
                ty: ref_array(),
            },
        ],
    });

    // (type $HamtCollision (struct (field $tag i32) (field $hash i64) (field $entries (ref $Array))))
    // Keep this layout distinct from HamtNode. Wasm GC type checks are structural,
    // so identical final struct shapes can make ref.test HamtNode succeed for a collision.
    m.types.push(TypeDef::Struct {
        name: "HamtCollision".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef::named("tag", ValType::I32),
            FieldDef::named("hash", ValType::I64),
            FieldDef {
                name: Some("entries".into()),
                mutable: false,
                ty: ref_array(),
            },
        ],
    });

    // (type $PDict (struct (field $size i32) (field $root (ref null $HamtNode)) (field $order (ref $PVec))))
    // PVec must be defined before PDict — placed above.
    m.types.push(TypeDef::Struct {
        name: "PDict".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef {
                name: Some("size".into()),
                mutable: true,
                ty: ValType::I32,
            },
            FieldDef {
                name: Some("root".into()),
                mutable: true,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named("HamtNode".into()),
                },
            },
            FieldDef {
                name: Some("order".into()),
                mutable: true,
                ty: ref_pvec(),
            },
        ],
    });

    // (type $ClosureEnv (array anyref))
    m.types.push(TypeDef::Array {
        name: "ClosureEnv".into(),
        elem: FieldDef {
            name: None,
            mutable: false,
            ty: ValType::Anyref,
        },
    });

    // (type $ClosureFunc (func (param anyref anyref) (result anyref)))
    // Universal closure signature: first param is env, second is a boxed-args anyref.
    // All user functions share this type so closures can be stored/called uniformly.
    m.types.push(TypeDef::FuncType {
        name: "ClosureFunc".into(),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::Anyref],
    });

    // (type $Closure (sub (struct ...))) — non-final to allow typed closure subtypes
    m.types.push(TypeDef::Struct {
        name: "Closure".into(),
        supertype: None,
        non_final: true,
        fields: vec![
            FieldDef {
                name: Some("func_ref".into()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named("ClosureFunc".into()),
                },
            },
            FieldDef {
                name: Some("env".into()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named("ClosureEnv".into()),
                },
            },
        ],
    });

    // (type $Variant (struct (field $type_id i32) (field $variant_id i32) (field $payload (ref null $Array))))
    m.types.push(TypeDef::Struct {
        name: "Variant".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef::named("type_id", ValType::I32),
            FieldDef::named("variant_id", ValType::I32),
            FieldDef {
                name: Some("payload".into()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named("Array".into()),
                },
            },
        ],
    });

    // (type $BoxedInt (struct (field $v i64)))
    m.types.push(TypeDef::Struct {
        name: "BoxedInt".into(),
        supertype: None,
        non_final: false,
        fields: vec![FieldDef::named("v", ValType::I64)],
    });

    // (type $BoxedFloat (struct (field $v f64)))
    m.types.push(TypeDef::Struct {
        name: "BoxedFloat".into(),
        supertype: None,
        non_final: false,
        fields: vec![FieldDef::named("v", ValType::F64)],
    });

    // (type $IterState (sub (struct (field $seed anyref) (field $step anyref))))
    // Base iterator state: holds seed and step closure as anyref.
    // Typed subtypes extend this with concrete seed/step fields.
    m.types.push(TypeDef::Struct {
        name: "IterState".into(),
        supertype: None,
        non_final: true,
        fields: vec![
            FieldDef {
                name: Some("seed".into()),
                mutable: false,
                ty: ValType::Anyref,
            },
            FieldDef {
                name: Some("step".into()),
                mutable: false,
                ty: ValType::Anyref,
            },
        ],
    });

    // Task<T> is a Wasm-owned handle carrying only an integer task id. The
    // scheduler lives in the JS host (JSPI binding) and keys records by this id.
    m.types.push(TypeDef::Struct {
        name: "Task".into(),
        supertype: None,
        non_final: true,
        fields: vec![FieldDef {
            name: Some("id".into()),
            mutable: false,
            ty: ValType::I32,
        }],
    });

    m
}
