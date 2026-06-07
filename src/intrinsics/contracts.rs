use crate::intrinsics::registry;
use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;

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
    pub abi_result: Option<IntrinsicAbiResult>,
}

pub fn contract(func_id: FuncId) -> Option<IntrinsicContract> {
    let spec = registry::spec(func_id)?;
    if !spec.include_in_contract_registry {
        return None;
    }

    Some(IntrinsicContract {
        func_id,
        twinkle_name: spec.twinkle_name,
        dispatch: spec.dispatch,
        abi_result: intrinsic_abi_result(func_id),
    })
}

fn intrinsic_abi_result(func_id: FuncId) -> Option<IntrinsicAbiResult> {
    if matches!(
        func_id,
        id if id == prelude_ids::INT_TO_STRING
            || id == prelude_ids::FLOAT_TO_STRING
            || id == prelude_ids::BOOL_TO_STRING
            || id == prelude_ids::STRING_TO_STRING
            || id == prelude_ids::STRING_SLICE
            || id == prelude_ids::BYTE_TO_STRING
            || id == prelude_ids::STRING_CONCAT
    ) {
        return Some(IntrinsicAbiResult::RefStringNullable);
    }

    if matches!(
        func_id,
        id if id == prelude_ids::BYTE_TO_INT
            || id == prelude_ids::CHAR_CODE_AT
            || id == prelude_ids::STRING_LEN
            || id == prelude_ids::VECTOR_LEN
            || id == prelude_ids::DICT_LEN
            || id == prelude_ids::DICT_HAS
    ) {
        return Some(IntrinsicAbiResult::I64);
    }

    if matches!(
        func_id,
        id if id == prelude_ids::VECTOR_APPEND
            || id == prelude_ids::VECTOR_CONCAT
            || id == prelude_ids::VECTOR_SLICE
            || id == prelude_ids::VECTOR_DROP_LAST
            || id == prelude_ids::VECTOR_GATHER
            || id == prelude_ids::VECTOR_SET_IN_PLACE
    ) {
        return Some(IntrinsicAbiResult::RefArrayNullable);
    }

    Some(IntrinsicAbiResult::Anyref)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn contract_presence_matches_registry_flag() {
        for spec in registry::all_specs() {
            let has_contract = contract(spec.func_id).is_some();
            assert_eq!(has_contract, spec.include_in_contract_registry);
        }
    }

    #[test]
    fn contract_registry_entries_have_abi_result() {
        for spec in registry::all_specs()
            .iter()
            .filter(|spec| spec.include_in_contract_registry)
        {
            let entry = contract(spec.func_id).expect("missing contract");
            assert!(
                entry.abi_result.is_some(),
                "missing ABI for {}",
                entry.twinkle_name
            );
        }
    }
}
