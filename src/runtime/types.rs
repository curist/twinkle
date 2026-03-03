use crate::wasm::ir::*;

/// Qualified type names as they appear *after* linking (namespace "rt.types" → prefix "rt_types").
/// All other runtime modules use these constants so their instruction operands are already
/// correctly qualified and do not need linker rewriting.
pub const T_ARRAY: &str = "rt_types__Array";
pub const T_STRING: &str = "rt_types__String";
pub const T_DICT_ENTRY: &str = "rt_types__DictEntry";
pub const T_DICT: &str = "rt_types__Dict";
pub const T_CLOSURE_ENV: &str = "rt_types__ClosureEnv";
pub const T_CLOSURE_FUNC: &str = "rt_types__ClosureFunc";
pub const T_CLOSURE: &str = "rt_types__Closure";
pub const T_VARIANT: &str = "rt_types__Variant";
pub const T_BOXED_INT: &str = "rt_types__BoxedInt";
pub const T_BOXED_FLOAT: &str = "rt_types__BoxedFloat";

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
/// Ref-to-Dict (non-null)
pub fn ref_dict() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_DICT.into()),
    }
}
/// Ref-to-Dict (nullable)
pub fn ref_dict_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_DICT.into()),
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

    // (type $DictEntry (struct (field $key anyref) (field $val anyref)))
    m.types.push(TypeDef::Struct {
        name: "DictEntry".into(),
        fields: vec![
            FieldDef::named("key", ValType::Anyref),
            FieldDef::named("val", ValType::Anyref),
        ],
    });

    // (type $Dict (array (mut (ref null $DictEntry))))
    m.types.push(TypeDef::Array {
        name: "Dict".into(),
        elem: FieldDef {
            name: None,
            mutable: true,
            ty: ValType::Ref {
                nullable: true,
                heap: HeapType::Named("DictEntry".into()),
            },
        },
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

    // (type $Closure (struct (field $func_ref (ref null $ClosureFunc)) (field $env (ref null $ClosureEnv))))
    m.types.push(TypeDef::Struct {
        name: "Closure".into(),
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
        fields: vec![FieldDef::named("v", ValType::I64)],
    });

    // (type $BoxedFloat (struct (field $v f64)))
    m.types.push(TypeDef::Struct {
        name: "BoxedFloat".into(),
        fields: vec![FieldDef::named("v", ValType::F64)],
    });

    m
}
