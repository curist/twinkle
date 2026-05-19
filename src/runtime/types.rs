use crate::wasm::ir::*;

/// Qualified type names as they appear *after* linking (namespace "rt.types" → prefix "rt_types").
/// All other runtime modules use these constants so their instruction operands are already
/// correctly qualified and do not need linker rewriting.
pub const T_ARRAY: &str = "rt_types__Array";
pub const T_STRING: &str = "rt_types__String";
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
pub const T_VEC_CHILDREN: &str = "rt_types__VecChildren";
pub const T_VEC_INTERNAL: &str = "rt_types__VecInternal";
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

    // (type $VecInternal (struct (field $children (ref $VecChildren))))
    m.types.push(TypeDef::Struct {
        name: "VecInternal".into(),
        supertype: None,
        non_final: false,
        fields: vec![FieldDef {
            name: Some("children".into()),
            mutable: false,
            ty: ref_vec_children(),
        }],
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
            FieldDef::named("size", ValType::I32),
            FieldDef {
                name: Some("root".into()),
                mutable: false,
                ty: ValType::Ref {
                    nullable: true,
                    heap: HeapType::Named("HamtNode".into()),
                },
            },
            FieldDef {
                name: Some("order".into()),
                mutable: false,
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

    m
}
