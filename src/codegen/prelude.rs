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

    pub(crate) fn intrinsic(twinkle_name: &'static str) -> Self {
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
        prelude_ids::EPRINT,
        PreludeEntry::runtime(
            "eprint",
            "rt.core",
            "eprint",
            "rt_core__eprint",
            vec![ref_string_null()],
            vec![],
        ),
    );
    map.insert(
        prelude_ids::EPRINTLN,
        PreludeEntry::runtime(
            "eprintln",
            "rt.core",
            "eprintln",
            "rt_core__eprintln",
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
        prelude_ids::VECTOR_LEN,
        PreludeEntry::runtime(
            "Vector.len",
            "rt.arr",
            "len",
            "rt_arr__len",
            vec![ref_array_null()],
            vec![ValType::I32],
        ),
    );
    map.insert(
        prelude_ids::VECTOR_PUSH,
        PreludeEntry::intrinsic("Vector.push"),
    );
    map.insert(
        prelude_ids::VECTOR_SET_UNSAFE,
        PreludeEntry::runtime(
            "Vector.set_unsafe",
            "rt.arr",
            "set",
            "rt_arr__set",
            vec![ref_array_null(), ValType::I32, ValType::Anyref],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::VECTOR_CONCAT,
        PreludeEntry::runtime(
            "Vector.concat",
            "rt.arr",
            "concat",
            "rt_arr__concat",
            vec![ref_array_null(), ref_array_null()],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::VECTOR_SLICE,
        PreludeEntry::runtime(
            "Vector.slice",
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
            "get_option",
            "rt_dict__get_option",
            vec![ref_dict_null(), ValType::Anyref],
            vec![ValType::Ref {
                nullable: false,
                heap: crate::wasm::ir::HeapType::Named(
                    crate::runtime::types::T_VARIANT.to_string(),
                ),
            }],
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
        prelude_ids::VECTOR_BUILDER_NEW,
        PreludeEntry::intrinsic("__vector_builder_new"),
    );
    map.insert(
        prelude_ids::VECTOR_BUILDER_PUSH,
        PreludeEntry::intrinsic("__vector_builder_push"),
    );
    map.insert(
        prelude_ids::VECTOR_BUILDER_FREEZE,
        PreludeEntry::intrinsic("__vector_builder_freeze"),
    );
    map.insert(
        prelude_ids::VECTOR_GET,
        PreludeEntry::intrinsic("Vector.get"),
    );
    map.insert(
        prelude_ids::VECTOR_SET,
        PreludeEntry::intrinsic("Vector.set"),
    );
    map.insert(
        prelude_ids::VECTOR_MAKE,
        PreludeEntry::intrinsic("Vector.make"),
    );
    map.insert(
        prelude_ids::DEBUG_STDIN_READ_ALL,
        PreludeEntry::intrinsic("__debug_stdin_read_all"),
    );
    map.insert(
        prelude_ids::DEBUG_READ_FILE,
        PreludeEntry::intrinsic("__debug_read_file"),
    );
    map.insert(
        prelude_ids::HOST_READ_FILE,
        PreludeEntry::runtime(
            "__host_read_file",
            "host",
            "read_file",
            "host_read_file",
            vec![ref_string_null()],
            vec![ref_string()],
        ),
    );
    map.insert(
        prelude_ids::HOST_WRITE_FILE,
        PreludeEntry::runtime(
            "__host_write_file",
            "host",
            "write_file",
            "host_write_file",
            vec![ref_string_null(), ref_string_null()],
            vec![],
        ),
    );
    map.insert(
        prelude_ids::HOST_WRITE_BYTES,
        PreludeEntry::runtime(
            "__host_write_bytes",
            "host",
            "write_bytes",
            "host_write_bytes",
            vec![ref_string_null(), ref_array_null()],
            vec![],
        ),
    );
    map.insert(
        prelude_ids::HOST_MKDIRP,
        PreludeEntry::runtime(
            "__host_mkdirp",
            "host",
            "mkdirp",
            "host_mkdirp",
            vec![ref_string_null()],
            vec![],
        ),
    );
    map.insert(
        prelude_ids::HOST_LIST_DIR,
        PreludeEntry::runtime(
            "__host_list_dir",
            "host",
            "list_dir",
            "host_list_dir",
            vec![ref_string_null()],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::HOST_EXISTS,
        PreludeEntry::runtime(
            "__host_exists",
            "host",
            "exists",
            "host_exists",
            vec![ref_string_null()],
            vec![ValType::I32],
        ),
    );
    map.insert(
        prelude_ids::HOST_ARGS,
        PreludeEntry::runtime(
            "__host_args",
            "host",
            "args",
            "host_args",
            vec![],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::HOST_ENV,
        PreludeEntry::runtime(
            "__host_env",
            "host",
            "env",
            "host_env",
            vec![ref_string_null()],
            vec![ref_array()],
        ),
    );
    map.insert(
        prelude_ids::HOST_CWD,
        PreludeEntry::runtime(
            "__host_cwd",
            "host",
            "cwd",
            "host_cwd",
            vec![],
            vec![ref_string()],
        ),
    );
    map.insert(
        prelude_ids::HOST_EXIT,
        PreludeEntry::runtime(
            "__host_exit",
            "host",
            "exit",
            "host_exit",
            vec![ValType::I64],
            vec![],
        ),
    );

    debug_assert!(map.contains_key(&prelude_ids::DEBUG_READ_FILE));
    debug_assert!(map.contains_key(&prelude_ids::HOST_EXIT));
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prelude_map_covers_all_fixed_ids() {
        let map = build_prelude_map();
        for id in 1..=prelude_ids::DEBUG_READ_FILE.0 {
            assert!(
                map.contains_key(&FuncId(id)),
                "missing prelude FuncId({id})"
            );
        }
        for id in [
            prelude_ids::VECTOR_GET.0,
            prelude_ids::VECTOR_SET.0,
            prelude_ids::VECTOR_MAKE.0,
            prelude_ids::EPRINT.0,
            prelude_ids::EPRINTLN.0,
            prelude_ids::HOST_READ_FILE.0,
            prelude_ids::HOST_WRITE_FILE.0,
            prelude_ids::HOST_WRITE_BYTES.0,
            prelude_ids::HOST_MKDIRP.0,
            prelude_ids::HOST_LIST_DIR.0,
            prelude_ids::HOST_EXISTS.0,
            prelude_ids::HOST_ARGS.0,
            prelude_ids::HOST_ENV.0,
            prelude_ids::HOST_CWD.0,
            prelude_ids::HOST_EXIT.0,
        ] {
            assert!(
                map.contains_key(&FuncId(id)),
                "missing host prelude FuncId({id})"
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
