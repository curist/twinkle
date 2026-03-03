use std::collections::HashMap;

use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;
use crate::runtime::types::{
    ref_array, ref_array_null, ref_dict, ref_dict_null, ref_string, ref_string_null,
};
use crate::wasm::ir::{FuncSym, ValType};

pub type PreludeMap = HashMap<FuncId, PreludeEntry>;

#[derive(Debug, Clone)]
pub struct PreludeEntry {
    pub twinkle_name: &'static str,
    pub runtime_module: Option<&'static str>,
    pub runtime_name: Option<&'static str>,
    pub runtime_sym: Option<FuncSym>,
    pub runtime_params: Vec<ValType>,
    pub runtime_results: Vec<ValType>,
}

impl PreludeEntry {
    fn runtime(
        twinkle_name: &'static str,
        runtime_module: &'static str,
        runtime_name: &'static str,
        runtime_sym: &'static str,
        runtime_params: Vec<ValType>,
        runtime_results: Vec<ValType>,
    ) -> Self {
        Self {
            twinkle_name,
            runtime_module: Some(runtime_module),
            runtime_name: Some(runtime_name),
            runtime_sym: Some(runtime_sym.to_string()),
            runtime_params,
            runtime_results,
        }
    }

    fn intrinsic(twinkle_name: &'static str) -> Self {
        Self {
            twinkle_name,
            runtime_module: None,
            runtime_name: None,
            runtime_sym: None,
            runtime_params: Vec::new(),
            runtime_results: Vec::new(),
        }
    }

    pub fn is_runtime_call(&self) -> bool {
        self.runtime_sym.is_some()
    }
}

pub fn build_prelude_map() -> PreludeMap {
    let mut map = HashMap::new();

    map.insert(
        prelude_ids::PRINT,
        PreludeEntry::runtime(
            "print",
            "rt.core",
            "print",
            "rt_core__print",
            vec![ref_string_null()],
            vec![],
        ),
    );
    map.insert(
        prelude_ids::PRINTLN,
        PreludeEntry::runtime(
            "println",
            "rt.core",
            "println",
            "rt_core__println",
            vec![ref_string_null()],
            vec![],
        ),
    );
    map.insert(
        prelude_ids::ERROR,
        PreludeEntry::runtime(
            "error",
            "rt.core",
            "trap",
            "rt_core__trap",
            vec![ref_string_null()],
            vec![],
        ),
    );

    map.insert(
        prelude_ids::INT_TO_STRING,
        PreludeEntry::runtime(
            "int_to_string",
            "rt.str",
            "from_i64",
            "rt_str__from_i64",
            vec![ValType::I64],
            vec![ref_string()],
        ),
    );
    map.insert(
        prelude_ids::FLOAT_TO_STRING,
        PreludeEntry::runtime(
            "float_to_string",
            "rt.str",
            "from_f64",
            "rt_str__from_f64",
            vec![ValType::F64],
            vec![ref_string()],
        ),
    );
    map.insert(
        prelude_ids::BOOL_TO_STRING,
        PreludeEntry::runtime(
            "bool_to_string",
            "rt.str",
            "from_bool",
            "rt_str__from_bool",
            vec![ValType::I32],
            vec![ref_string()],
        ),
    );
    map.insert(
        prelude_ids::STRING_TO_STRING,
        PreludeEntry::intrinsic("string_to_string"),
    );

    map.insert(
        prelude_ids::STRING_LEN,
        PreludeEntry::runtime(
            "String.len",
            "rt.str",
            "len",
            "rt_str__len",
            vec![ref_string_null()],
            vec![ValType::I32],
        ),
    );
    map.insert(
        prelude_ids::STRING_CONCAT,
        PreludeEntry::runtime(
            "String.concat",
            "rt.str",
            "concat",
            "rt_str__concat",
            vec![ref_string_null(), ref_string_null()],
            vec![ref_string()],
        ),
    );
    map.insert(
        prelude_ids::STRING_SUBSTR,
        PreludeEntry::runtime(
            "String.substring",
            "rt.str",
            "substring",
            "rt_str__substring",
            vec![ref_string_null(), ValType::I32, ValType::I32],
            vec![ref_string()],
        ),
    );

    map.insert(
        prelude_ids::ARRAY_LEN,
        PreludeEntry::runtime(
            "Array.len",
            "rt.arr",
            "len",
            "rt_arr__len",
            vec![ref_array_null()],
            vec![ValType::I32],
        ),
    );
    map.insert(
        prelude_ids::ARRAY_APPEND,
        PreludeEntry::intrinsic("Array.append"),
    );
    map.insert(
        prelude_ids::ARRAY_SET,
        PreludeEntry::runtime(
            "Array.set",
            "rt.arr",
            "set",
            "rt_arr__set",
            vec![ref_array_null(), ValType::I32, ValType::Anyref],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::ARRAY_CONCAT,
        PreludeEntry::runtime(
            "Array.concat",
            "rt.arr",
            "concat",
            "rt_arr__concat",
            vec![ref_array_null(), ref_array_null()],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::ARRAY_SLICE,
        PreludeEntry::runtime(
            "Array.slice",
            "rt.arr",
            "slice",
            "rt_arr__slice",
            vec![ref_array_null(), ValType::I32, ValType::I32],
            vec![ref_array()],
        ),
    );

    map.insert(
        prelude_ids::DICT_SET,
        PreludeEntry::runtime(
            "Dict.set",
            "rt.dict",
            "set",
            "rt_dict__set",
            vec![ref_dict_null(), ValType::Anyref, ValType::Anyref],
            vec![ref_dict()],
        ),
    );
    map.insert(
        prelude_ids::DICT_KEYS,
        PreludeEntry::runtime(
            "Dict.keys",
            "rt.dict",
            "keys",
            "rt_dict__keys",
            vec![ref_dict_null()],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::DICT_GET,
        PreludeEntry::runtime(
            "dict_get",
            "rt.dict",
            "get",
            "rt_dict__get",
            vec![ref_dict_null(), ValType::Anyref],
            vec![ValType::Anyref],
        ),
    );
    map.insert(
        prelude_ids::DICT_NEW,
        PreludeEntry::runtime(
            "Dict.new",
            "rt.dict",
            "make",
            "rt_dict__make",
            vec![],
            vec![ref_dict()],
        ),
    );
    map.insert(
        prelude_ids::DICT_LEN,
        PreludeEntry::runtime(
            "Dict.len",
            "rt.dict",
            "len",
            "rt_dict__len",
            vec![ref_dict_null()],
            vec![ValType::I32],
        ),
    );
    map.insert(
        prelude_ids::DICT_HAS,
        PreludeEntry::runtime(
            "Dict.has",
            "rt.dict",
            "has",
            "rt_dict__has",
            vec![ref_dict_null(), ValType::Anyref],
            vec![ValType::I32],
        ),
    );
    map.insert(
        prelude_ids::DICT_REMOVE,
        PreludeEntry::runtime(
            "Dict.remove",
            "rt.dict",
            "remove",
            "rt_dict__remove",
            vec![ref_dict_null(), ValType::Anyref],
            vec![ref_dict()],
        ),
    );

    map.insert(
        prelude_ids::RANGE_FROM,
        PreludeEntry::intrinsic("range_from"),
    );
    map.insert(prelude_ids::RANGE, PreludeEntry::intrinsic("range"));
    map.insert(
        prelude_ids::RANGE_STEP,
        PreludeEntry::intrinsic("range_step"),
    );
    map.insert(prelude_ids::CELL_NEW, PreludeEntry::intrinsic("Cell.new"));
    map.insert(prelude_ids::CELL_GET, PreludeEntry::intrinsic("Cell.get"));
    map.insert(prelude_ids::CELL_SET, PreludeEntry::intrinsic("Cell.set"));
    map.insert(
        prelude_ids::CELL_UPDATE,
        PreludeEntry::intrinsic("Cell.update"),
    );
    map.insert(
        prelude_ids::DICT_GET_UNSAFE,
        PreludeEntry::intrinsic("dict_get_unsafe"),
    );
    map.insert(
        prelude_ids::ITERATOR_NEXT,
        PreludeEntry::intrinsic("Iterator.next"),
    );
    map.insert(
        prelude_ids::ITERATOR_UNFOLD,
        PreludeEntry::intrinsic("Iterator.unfold"),
    );
    map.insert(
        prelude_ids::ARRAY_BUILDER_NEW,
        PreludeEntry::intrinsic("__array_builder_new"),
    );
    map.insert(
        prelude_ids::ARRAY_BUILDER_PUSH,
        PreludeEntry::intrinsic("__array_builder_push"),
    );
    map.insert(
        prelude_ids::ARRAY_BUILDER_FREEZE,
        PreludeEntry::intrinsic("__array_builder_freeze"),
    );
    map.insert(
        prelude_ids::DEBUG_STDIN_READ_ALL,
        PreludeEntry::intrinsic("__debug_stdin_read_all"),
    );
    map.insert(
        prelude_ids::DEBUG_READ_FILE,
        PreludeEntry::intrinsic("__debug_read_file"),
    );

    debug_assert_eq!(map.len(), prelude_ids::DEBUG_READ_FILE.0 as usize);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_map_covers_all_fixed_ids() {
        let map = build_prelude_map();
        let max_id = prelude_ids::DEBUG_READ_FILE.0;
        for id in 1..=max_id {
            assert!(
                map.contains_key(&FuncId(id)),
                "missing prelude FuncId({id})"
            );
        }
    }

    #[test]
    fn runtime_entries_have_import_metadata() {
        let map = build_prelude_map();
        for (func_id, entry) in map {
            if entry.is_runtime_call() {
                assert!(
                    entry.runtime_module.is_some() && entry.runtime_name.is_some(),
                    "runtime entry FuncId({}) missing module/name metadata",
                    func_id.0
                );
            }
        }
    }
}
