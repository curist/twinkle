use crate::intrinsics::registry;
use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;
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

pub fn function_signatures() -> Vec<FunctionSignature> {
    prelude_signature_ids()
        .iter()
        .filter_map(|func_id| {
            let entry = contract(*func_id)?;
            Some(FunctionSignature {
                name: entry.twinkle_name.to_string(),
                type_params: entry.type_params,
                params: entry.params,
                ret: Some(entry.ret),
            })
        })
        .collect()
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
