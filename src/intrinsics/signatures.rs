use std::collections::HashMap;
use std::sync::OnceLock;

use crate::intrinsics::registry;
use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;
use crate::syntax::{ast::Item, parse_source};
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::resolve::Resolver;
use crate::types::ty::{
    CELL_TYPE_ID, FunctionSignature, ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID,
    RANGE_TYPE_ID, UNFOLD_STEP_TYPE_ID,
};

pub use crate::intrinsics::registry::IntrinsicDispatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicAbiResult {
    Anyref,
    I64,
    RefStringNullable,
    RefArrayNullable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrinsicContract {
    pub func_id: FuncId,
    pub twinkle_name: &'static str,
    pub dispatch: IntrinsicDispatch,
    pub type_params: Vec<String>,
    pub params: Vec<MonoType>,
    pub ret: MonoType,
    pub abi_result: Option<IntrinsicAbiResult>,
}

pub fn contract(func_id: FuncId) -> Option<IntrinsicContract> {
    match func_id {
        id if id == prelude_ids::INT_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Int.to_string",
            dispatch: IntrinsicDispatch::Runtime,
            type_params: vec![],
            params: vec![MonoType::Int],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::FLOAT_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Float.to_string",
            dispatch: IntrinsicDispatch::Runtime,
            type_params: vec![],
            params: vec![MonoType::Float],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::BOOL_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Bool.to_string",
            dispatch: IntrinsicDispatch::Runtime,
            type_params: vec![],
            params: vec![MonoType::Bool],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::STRING_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.to_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::STRING_GET => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.get",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String, MonoType::Int],
            ret: option_ty(MonoType::Byte),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::STRING_SLICE => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.slice",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String, MonoType::Int, MonoType::Int],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::BYTE_TO_INT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Byte.to_int",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Byte],
            ret: MonoType::Int,
            abi_result: Some(IntrinsicAbiResult::I64),
        }),
        id if id == prelude_ids::BYTE_FROM_INT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Byte.from_int",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Int],
            ret: option_ty(MonoType::Byte),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::BYTE_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Byte.to_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Byte],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::CHAR_CODE_AT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.char_code_at",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String, MonoType::Int],
            ret: MonoType::Int,
            abi_result: Some(IntrinsicAbiResult::I64),
        }),
        id if id == prelude_ids::FROM_CHAR_CODE => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.from_char_code",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Int],
            ret: option_ty(MonoType::String),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::FROM_CODE_POINT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.from_code_point",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Int],
            ret: option_ty(MonoType::String),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::STRING_UTF8_BYTES => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.utf8_bytes",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String],
            ret: MonoType::Vector(Box::new(MonoType::Byte)),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::STRING_FROM_UTF8 => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.from_utf8",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Vector(Box::new(MonoType::Byte))],
            ret: option_ty(MonoType::String),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::INT_FROM_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Int.from_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String],
            ret: option_ty(MonoType::Int),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::FLOAT_FROM_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Float.from_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::String],
            ret: option_ty(MonoType::Float),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::RANGE_FROM => Some(IntrinsicContract {
            func_id,
            twinkle_name: "range_from",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Int, MonoType::Int],
            ret: range_ty(),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::RANGE => Some(IntrinsicContract {
            func_id,
            twinkle_name: "range",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Int],
            ret: range_ty(),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::RANGE_STEP => Some(IntrinsicContract {
            func_id,
            twinkle_name: "range_step",
            dispatch: IntrinsicDispatch::Intrinsic,
            type_params: vec![],
            params: vec![MonoType::Int, MonoType::Int, MonoType::Int],
            ret: range_ty(),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::CELL_NEW => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Cell.new",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![t.clone()],
                ret: cell_ty(t),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::CELL_GET => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Cell.get",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![cell_ty(t.clone())],
                ret: t,
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::CELL_SET => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Cell.set",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![cell_ty(t.clone()), t],
                ret: MonoType::Void,
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::CELL_UPDATE => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Cell.update",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![
                    cell_ty(t.clone()),
                    MonoType::Function {
                        params: vec![t.clone()],
                        ret: Box::new(t),
                    },
                ],
                ret: MonoType::Void,
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::DICT_GET_UNSAFE => {
            let k = ty_var("K");
            let v = ty_var("V");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "dict_get_unsafe",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![MonoType::Dict(Box::new(k.clone()), Box::new(v.clone())), k],
                ret: v,
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::ITERATOR_NEXT => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Iterator.next",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![iterator_ty(t.clone())],
                ret: option_ty(iter_item_ty(t)),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::ITERATOR_UNFOLD => {
            let t = ty_var("T");
            let s = ty_var("S");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Iterator.unfold",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string(), "S".to_string()],
                params: vec![
                    s.clone(),
                    MonoType::Function {
                        params: vec![s.clone()],
                        ret: Box::new(unfold_step_ty(t.clone(), s.clone())),
                    },
                ],
                ret: iterator_ty(t),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::VECTOR_PUSH => {
            let t = ty_var("T");
            let vec_t = MonoType::Vector(Box::new(t.clone()));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.push",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![vec_t.clone(), t],
                ret: vec_t,
                abi_result: Some(IntrinsicAbiResult::RefArrayNullable),
            })
        }
        id if id == prelude_ids::VECTOR_GET => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.get",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![MonoType::Vector(Box::new(t.clone())), MonoType::Int],
                ret: option_ty(t),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::VECTOR_SET => {
            let t = ty_var("T");
            let vec_t = MonoType::Vector(Box::new(t.clone()));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.set",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![vec_t.clone(), MonoType::Int, t],
                ret: option_ty(vec_t),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::VECTOR_MAKE => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.make",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![MonoType::Int, t.clone()],
                ret: MonoType::Vector(Box::new(t)),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::STRING_LEN => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.len",
            dispatch: IntrinsicDispatch::Runtime,
            type_params: vec![],
            params: vec![MonoType::String],
            ret: MonoType::Int,
            abi_result: Some(IntrinsicAbiResult::I64),
        }),
        id if id == prelude_ids::STRING_CONCAT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.concat",
            dispatch: IntrinsicDispatch::Runtime,
            type_params: vec![],
            params: vec![MonoType::String, MonoType::String],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::VECTOR_LEN => {
            let t = ty_var("T");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.len",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["T".to_string()],
                params: vec![MonoType::Vector(Box::new(t))],
                ret: MonoType::Int,
                abi_result: Some(IntrinsicAbiResult::I64),
            })
        }
        id if id == prelude_ids::VECTOR_CONCAT => {
            let t = ty_var("T");
            let vec_t = MonoType::Vector(Box::new(t));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.concat",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["T".to_string()],
                params: vec![vec_t.clone(), vec_t.clone()],
                ret: vec_t,
                abi_result: Some(IntrinsicAbiResult::RefArrayNullable),
            })
        }
        id if id == prelude_ids::VECTOR_SLICE => {
            let t = ty_var("T");
            let vec_t = MonoType::Vector(Box::new(t));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Vector.slice",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["T".to_string()],
                params: vec![vec_t.clone(), MonoType::Int, MonoType::Int],
                ret: vec_t,
                abi_result: Some(IntrinsicAbiResult::RefArrayNullable),
            })
        }
        id if id == prelude_ids::DICT_SET => {
            let k = ty_var("K");
            let v = ty_var("V");
            let dict_kv = MonoType::Dict(Box::new(k.clone()), Box::new(v.clone()));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Dict.set",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![dict_kv.clone(), k, v],
                ret: dict_kv,
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::DICT_KEYS => {
            let k = ty_var("K");
            let v = ty_var("V");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Dict.keys",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![MonoType::Dict(Box::new(k.clone()), Box::new(v))],
                ret: MonoType::Vector(Box::new(k)),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::DICT_NEW => {
            let k = ty_var("K");
            let v = ty_var("V");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Dict.new",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![],
                ret: MonoType::Dict(Box::new(k), Box::new(v)),
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::DICT_LEN => {
            let k = ty_var("K");
            let v = ty_var("V");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Dict.len",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![MonoType::Dict(Box::new(k), Box::new(v))],
                ret: MonoType::Int,
                abi_result: Some(IntrinsicAbiResult::I64),
            })
        }
        id if id == prelude_ids::DICT_HAS => {
            let k = ty_var("K");
            let v = ty_var("V");
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Dict.has",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![MonoType::Dict(Box::new(k.clone()), Box::new(v)), k],
                ret: MonoType::Bool,
                abi_result: Some(IntrinsicAbiResult::I64),
            })
        }
        id if id == prelude_ids::DICT_REMOVE => {
            let k = ty_var("K");
            let v = ty_var("V");
            let dict_kv = MonoType::Dict(Box::new(k.clone()), Box::new(v));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "Dict.remove",
                dispatch: IntrinsicDispatch::Runtime,
                type_params: vec!["K".to_string(), "V".to_string()],
                params: vec![dict_kv.clone(), k],
                ret: dict_kv,
                abi_result: Some(IntrinsicAbiResult::Anyref),
            })
        }
        id if id == prelude_ids::VECTOR_SET_IN_PLACE => {
            let t = ty_var("T");
            let vec_t = MonoType::Vector(Box::new(t.clone()));
            Some(IntrinsicContract {
                func_id,
                twinkle_name: "__vector_set_in_place",
                dispatch: IntrinsicDispatch::Intrinsic,
                type_params: vec!["T".to_string()],
                params: vec![vec_t.clone(), MonoType::Int, t],
                ret: vec_t,
                abi_result: Some(IntrinsicAbiResult::RefArrayNullable),
            })
        }
        _ => None,
    }
}

pub fn twinkle_name(func_id: FuncId) -> Option<&'static str> {
    registry::spec(func_id).map(|spec| spec.twinkle_name)
}

pub fn prelude_signature_ids() -> &'static [FuncId] {
    registry::signature_func_ids()
}

struct SignatureSourceModule {
    virtual_path: &'static str,
    module_alias: Option<&'static str>,
    source: &'static str,
}

const SIGNATURE_SOURCE_MODULES: &[SignatureSourceModule] = &[
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/int.tw",
        module_alias: Some("Int"),
        source: include_str!("../../prelude/signatures/int.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/float.tw",
        module_alias: Some("Float"),
        source: include_str!("../../prelude/signatures/float.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/bool.tw",
        module_alias: Some("Bool"),
        source: include_str!("../../prelude/signatures/bool.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/string.tw",
        module_alias: Some("String"),
        source: include_str!("../../prelude/signatures/string.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/vector.tw",
        module_alias: Some("Vector"),
        source: include_str!("../../prelude/signatures/vector.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/dict.tw",
        module_alias: Some("Dict"),
        source: include_str!("../../prelude/signatures/dict.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/cell.tw",
        module_alias: Some("Cell"),
        source: include_str!("../../prelude/signatures/cell.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/iterator.tw",
        module_alias: Some("Iterator"),
        source: include_str!("../../prelude/signatures/iterator.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/byte.tw",
        module_alias: Some("Byte"),
        source: include_str!("../../prelude/signatures/byte.tw"),
    },
    SignatureSourceModule {
        virtual_path: "/virtual/prelude/signatures/range.tw",
        module_alias: None,
        source: include_str!("../../prelude/signatures/range.tw"),
    },
];

fn load_signature_map_from_tw_sources() -> HashMap<String, FunctionSignature> {
    let mut signatures = HashMap::new();

    for module in SIGNATURE_SOURCE_MODULES {
        let (ast, _) = parse_source(module.source, module.virtual_path).unwrap_or_else(|err| {
            panic!(
                "failed to parse intrinsic signatures from {}: {err}",
                module.virtual_path
            )
        });
        let resolved = Resolver::resolve(
            &ast,
            TypeEnv::new(),
            ValueEnv::new_without_intrinsic_signatures(),
        )
        .unwrap_or_else(|errs| {
            panic!(
                "failed to resolve intrinsic signatures from {}: {:?}",
                module.virtual_path, errs
            )
        });

        for item in &ast.items {
            let Item::Function(decl) = item else {
                continue;
            };
            let mut sig = resolved
                .value_env
                .get_function(&decl.name)
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "signature '{}' missing from resolved intrinsic source {}",
                        decl.name, module.virtual_path
                    )
                });
            if let Some(alias) = module.module_alias {
                sig.name = format!("{}.{}", alias, sig.name);
            }
            signatures.insert(sig.name.clone(), sig);
        }
    }

    signatures
}

pub fn function_signatures() -> Vec<FunctionSignature> {
    static SIGNATURES: OnceLock<Vec<FunctionSignature>> = OnceLock::new();
    SIGNATURES
        .get_or_init(|| {
            let by_name = load_signature_map_from_tw_sources();
            prelude_signature_ids()
                .iter()
                .map(|func_id| {
                    let spec = registry::spec(*func_id)
                        .expect("missing registry spec for signature FuncId");
                    let mut sig = by_name.get(spec.twinkle_name).cloned().unwrap_or_else(|| {
                        panic!(
                            "intrinsic signature '{}' missing from .tw signature sources",
                            spec.twinkle_name
                        )
                    });
                    sig.doc = builtin_doc(spec.twinkle_name).map(str::to_string);
                    sig
                })
                .collect()
        })
        .clone()
}

/// Hard-coded doc strings for builtin and intrinsic functions.
fn builtin_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        // Int
        "Int.to_string" => "Convert an integer to its string representation.",
        "Int.abs" => "Return the absolute value.",
        "Int.min" => "Return the smaller of two integers.",
        "Int.max" => "Return the larger of two integers.",
        "Int.parse" => "Parse a string as an integer. Returns `Int?`.",

        // Float
        "Float.to_string" => "Convert a float to its string representation.",
        "Float.floor" => "Round toward negative infinity.",
        "Float.ceil" => "Round toward positive infinity.",
        "Float.round" => "Round to the nearest integer.",
        "Float.abs" => "Return the absolute value.",
        "Float.parse" => "Parse a string as a float. Returns `Float?`.",
        "Float.from_int" => "Convert an integer to a float.",
        "Float.min" => "Return the smaller of two floats.",
        "Float.max" => "Return the larger of two floats.",

        // Bool
        "Bool.to_string" => "Convert a boolean to \"true\" or \"false\".",

        // String
        "String.len" => "Return the byte length of the string.",
        "String.concat" => "Concatenate two strings.",
        "String.to_string" => "Return the string unchanged (identity).",
        "String.get" => "Return the byte at the given index, or `None` if out of bounds.",
        "String.slice" => "Return a substring by byte offsets. Traps on invalid UTF-8 boundary.",
        "String.substring" => "Return a substring by byte offsets (no boundary check).",
        "String.utf8_bytes" => "Copy the string's UTF-8 bytes into a `Vector<Byte>`.",
        "String.from_utf8" => "Validate UTF-8 bytes and create a string. Returns `String?`.",
        "String.from_code_point" => "Create a string from a Unicode code point. Returns `String?`.",

        // Byte
        "Byte.to_int" => "Convert a byte to its integer value (0–255).",
        "Byte.from_int" => "Create a byte from an integer (mod 256). Returns `Byte?`.",
        "Byte.to_string" => "Convert a byte to its string representation.",

        // Vector
        "Vector.len" => "Return the number of elements.",
        "Vector.push" => "Return a new vector with the element appended.",
        "Vector.concat" => "Return a new vector with all elements from both vectors.",
        "Vector.slice" => "Return a sub-vector from start (inclusive) to end (exclusive).",
        "Vector.get" => "Return the element at the given index, or `None` if out of bounds.",
        "Vector.set" => "Return a new vector with the element at the given index replaced.",
        "Vector.make" => "Create a vector of `n` copies of a value.",

        // Dict
        "Dict.new" => "Create an empty dictionary.",
        "Dict.set" => "Return a new dict with the key-value pair inserted or updated.",
        "Dict.keys" => "Return a vector of all keys.",
        "Dict.len" => "Return the number of key-value pairs.",
        "Dict.has" => "Return whether the key exists.",
        "Dict.remove" => "Return a new dict with the key removed.",

        // Cell
        "Cell.new" => "Create a mutable cell containing a value.",
        "Cell.get" => "Read the current value in the cell.",
        "Cell.set" => "Replace the value in the cell.",
        "Cell.update" => "Apply a function to the cell's value and store the result.",

        // Range
        "range" => "Create a range from 0 to `n` (exclusive).",
        "range_from" => "Create a range from `start` to `end` (exclusive).",
        "range_step" => "Create a range from `start` to `end` with a custom step.",

        // Iterator
        "Iterator.next" => "Advance the iterator and return the next value, or `None`.",
        "Iterator.unfold" => "Create an iterator from a seed and step function.",

        _ => return None,
    })
}

fn ty_var(name: &str) -> MonoType {
    MonoType::Var(name.to_string())
}

fn option_ty(inner: MonoType) -> MonoType {
    MonoType::Named {
        type_id: OPTION_TYPE_ID,
        args: vec![inner],
    }
}

fn range_ty() -> MonoType {
    MonoType::Named {
        type_id: RANGE_TYPE_ID,
        args: vec![],
    }
}

fn cell_ty(inner: MonoType) -> MonoType {
    MonoType::Named {
        type_id: CELL_TYPE_ID,
        args: vec![inner],
    }
}

fn iterator_ty(inner: MonoType) -> MonoType {
    MonoType::Named {
        type_id: ITERATOR_TYPE_ID,
        args: vec![inner],
    }
}

fn iter_item_ty(inner: MonoType) -> MonoType {
    MonoType::Named {
        type_id: ITER_ITEM_TYPE_ID,
        args: vec![inner],
    }
}

fn unfold_step_ty(yield_ty: MonoType, seed_ty: MonoType) -> MonoType {
    MonoType::Named {
        type_id: UNFOLD_STEP_TYPE_ID,
        args: vec![yield_ty, seed_ty],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intrinsics::registry;

    #[test]
    fn signature_registry_has_unique_names_and_ids() {
        let mut names = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for func_id in prelude_signature_ids() {
            let entry = contract(*func_id).expect("missing contract");
            assert!(names.insert(entry.twinkle_name));
            assert!(ids.insert(entry.func_id.0));
        }
    }

    #[test]
    fn signature_ids_match_canonical_registry() {
        let expected_ids: Vec<_> = registry::all_specs()
            .iter()
            .filter(|spec| spec.include_in_signature_registry)
            .map(|spec| spec.func_id)
            .collect();
        assert_eq!(prelude_signature_ids(), expected_ids.as_slice());
    }

    #[test]
    fn canonical_registry_matches_contract_name_and_dispatch() {
        for spec in registry::all_specs()
            .iter()
            .filter(|spec| spec.include_in_contract_registry)
        {
            let entry = contract(spec.func_id).expect("missing contract");
            assert_eq!(entry.twinkle_name, spec.twinkle_name);
            assert_eq!(entry.dispatch, spec.dispatch);
        }
    }
}
