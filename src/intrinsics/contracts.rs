use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;
use crate::types::ty::{FunctionSignature, MonoType, OPTION_TYPE_ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicDispatch {
    Runtime,
    Intrinsic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicAbiResult {
    Anyref,
    I64,
    RefStringNullable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrinsicContract {
    pub func_id: FuncId,
    pub twinkle_name: &'static str,
    pub dispatch: IntrinsicDispatch,
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
            params: vec![MonoType::Int],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::FLOAT_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Float.to_string",
            dispatch: IntrinsicDispatch::Runtime,
            params: vec![MonoType::Float],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::BOOL_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Bool.to_string",
            dispatch: IntrinsicDispatch::Runtime,
            params: vec![MonoType::Bool],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::STRING_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.to_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::STRING_GET => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.get",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String, MonoType::Int],
            ret: option_ty(MonoType::Byte),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::STRING_SLICE => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.slice",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String, MonoType::Int, MonoType::Int],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::BYTE_TO_INT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Byte.to_int",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::Byte],
            ret: MonoType::Int,
            abi_result: Some(IntrinsicAbiResult::I64),
        }),
        id if id == prelude_ids::BYTE_FROM_INT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Byte.from_int",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::Int],
            ret: option_ty(MonoType::Byte),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::BYTE_TO_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Byte.to_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::Byte],
            ret: MonoType::String,
            abi_result: Some(IntrinsicAbiResult::RefStringNullable),
        }),
        id if id == prelude_ids::CHAR_CODE_AT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.char_code_at",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String, MonoType::Int],
            ret: MonoType::Int,
            abi_result: Some(IntrinsicAbiResult::I64),
        }),
        id if id == prelude_ids::FROM_CHAR_CODE => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.from_char_code",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::Int],
            ret: option_ty(MonoType::String),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::FROM_CODE_POINT => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.from_code_point",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::Int],
            ret: option_ty(MonoType::String),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::STRING_UTF8_BYTES => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.utf8_bytes",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String],
            ret: MonoType::Vector(Box::new(MonoType::Byte)),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::STRING_FROM_UTF8 => Some(IntrinsicContract {
            func_id,
            twinkle_name: "String.from_utf8",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::Vector(Box::new(MonoType::Byte))],
            ret: option_ty(MonoType::String),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::INT_FROM_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Int.from_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String],
            ret: option_ty(MonoType::Int),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        id if id == prelude_ids::FLOAT_FROM_STRING => Some(IntrinsicContract {
            func_id,
            twinkle_name: "Float.from_string",
            dispatch: IntrinsicDispatch::Intrinsic,
            params: vec![MonoType::String],
            ret: option_ty(MonoType::Float),
            abi_result: Some(IntrinsicAbiResult::Anyref),
        }),
        _ => None,
    }
}

pub fn twinkle_name(func_id: FuncId) -> Option<&'static str> {
    contract(func_id).map(|entry| entry.twinkle_name)
}

pub fn prelude_signature_ids() -> &'static [FuncId] {
    &[
        prelude_ids::INT_TO_STRING,
        prelude_ids::FLOAT_TO_STRING,
        prelude_ids::BOOL_TO_STRING,
        prelude_ids::STRING_TO_STRING,
        prelude_ids::STRING_GET,
        prelude_ids::STRING_SLICE,
        prelude_ids::BYTE_TO_INT,
        prelude_ids::BYTE_FROM_INT,
        prelude_ids::BYTE_TO_STRING,
        prelude_ids::CHAR_CODE_AT,
        prelude_ids::FROM_CHAR_CODE,
        prelude_ids::FROM_CODE_POINT,
        prelude_ids::STRING_UTF8_BYTES,
        prelude_ids::STRING_FROM_UTF8,
        prelude_ids::INT_FROM_STRING,
        prelude_ids::FLOAT_FROM_STRING,
    ]
}

pub fn function_signatures() -> Vec<FunctionSignature> {
    prelude_signature_ids()
        .iter()
        .filter_map(|func_id| {
            let entry = contract(*func_id)?;
            Some(FunctionSignature {
                name: entry.twinkle_name.to_string(),
                type_params: vec![],
                params: entry.params,
                ret: Some(entry.ret),
            })
        })
        .collect()
}

fn option_ty(inner: MonoType) -> MonoType {
    MonoType::Named {
        type_id: OPTION_TYPE_ID,
        args: vec![inner],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
