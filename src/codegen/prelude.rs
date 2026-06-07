use std::collections::HashMap;

use crate::intrinsics::registry::{self, IntrinsicDispatch};
use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;
use crate::runtime::types::{
    T_VARIANT, ref_array, ref_array_null, ref_pdict, ref_pdict_null, ref_pvec, ref_pvec_null,
    ref_str_builder, ref_str_builder_null, ref_string, ref_string_null,
};
use crate::wasm::ir::{FuncSym, HeapType, ValType};

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

fn runtime_entry(func_id: FuncId, twinkle_name: &'static str) -> Option<PreludeEntry> {
    match func_id {
        id if id == prelude_ids::PRINT => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.core",
            "print",
            "rt_core__print",
            vec![ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::PRINTLN => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.core",
            "println",
            "rt_core__println",
            vec![ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::ERROR => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.core",
            "trap",
            "rt_core__trap",
            vec![ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::EPRINT => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.core",
            "eprint",
            "rt_core__eprint",
            vec![ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::EPRINTLN => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.core",
            "eprintln",
            "rt_core__eprintln",
            vec![ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::INT_TO_STRING => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "from_i64",
            "rt_str__from_i64",
            vec![ValType::I64],
            vec![ref_string()],
        )),
        id if id == prelude_ids::FLOAT_TO_STRING => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "from_f64",
            "rt_str__from_f64",
            vec![ValType::F64],
            vec![ref_string()],
        )),
        id if id == prelude_ids::BOOL_TO_STRING => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "from_bool",
            "rt_str__from_bool",
            vec![ValType::I32],
            vec![ref_string()],
        )),
        id if id == prelude_ids::STRING_LEN => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "len",
            "rt_str__len",
            vec![ref_string_null()],
            vec![ValType::I32],
        )),
        id if id == prelude_ids::STRING_CONCAT => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "concat",
            "rt_str__concat",
            vec![ref_string_null(), ref_string_null()],
            vec![ref_string()],
        )),
        id if id == prelude_ids::VECTOR_LEN => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "len",
            "rt_arr__len",
            vec![ref_pvec_null()],
            vec![ValType::I32],
        )),
        id if id == prelude_ids::VECTOR_SET_UNSAFE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "set",
            "rt_arr__set",
            vec![ref_pvec_null(), ValType::I32, ValType::Anyref],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::VECTOR_CONCAT => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "concat",
            "rt_arr__concat",
            vec![ref_pvec_null(), ref_pvec_null()],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::VECTOR_SLICE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "slice",
            "rt_arr__slice",
            vec![ref_pvec_null(), ValType::I32, ValType::I32],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::VECTOR_DROP_LAST => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "drop_last",
            "rt_arr__drop_last",
            vec![ref_pvec_null()],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::VECTOR_GATHER => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "gather",
            "rt_arr__gather",
            vec![ref_pvec_null(), ref_pvec_null()],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::DICT_SET => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "set",
            "rt_dict__set",
            vec![ref_pdict_null(), ValType::Anyref, ValType::Anyref],
            vec![ref_pdict()],
        )),
        id if id == prelude_ids::DICT_KEYS => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "keys",
            "rt_dict__keys",
            vec![ref_pdict_null()],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::DICT_GET => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "get_option",
            "rt_dict__get_option",
            vec![ref_pdict_null(), ValType::Anyref],
            vec![ValType::Ref {
                nullable: false,
                heap: HeapType::Named(T_VARIANT.to_string()),
            }],
        )),
        id if id == prelude_ids::DICT_NEW => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "make",
            "rt_dict__make",
            vec![],
            vec![ref_pdict()],
        )),
        id if id == prelude_ids::DICT_LEN => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "len",
            "rt_dict__len",
            vec![ref_pdict_null()],
            vec![ValType::I32],
        )),
        id if id == prelude_ids::DICT_HAS => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "has",
            "rt_dict__has",
            vec![ref_pdict_null(), ValType::Anyref],
            vec![ValType::I32],
        )),
        id if id == prelude_ids::DICT_REMOVE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "remove",
            "rt_dict__remove",
            vec![ref_pdict_null(), ValType::Anyref],
            vec![ref_pdict()],
        )),
        id if id == prelude_ids::DICT_SET_IN_PLACE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "set_in_place",
            "rt_dict__set_in_place",
            vec![ref_pdict_null(), ValType::Anyref, ValType::Anyref],
            vec![ref_pdict()],
        )),
        id if id == prelude_ids::DICT_REMOVE_IN_PLACE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.dict",
            "remove_in_place",
            "rt_dict__remove_in_place",
            vec![ref_pdict_null(), ValType::Anyref],
            vec![ref_pdict()],
        )),
        id if id == prelude_ids::VECTOR_BUILDER_NEW => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "builder_new",
            "rt_arr__builder_new",
            vec![],
            vec![ref_array_null()],
        )),
        id if id == prelude_ids::VECTOR_BUILDER_PUSH => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "builder_push",
            "rt_arr__builder_push",
            vec![ref_array_null(), ValType::Anyref],
            vec![],
        )),
        id if id == prelude_ids::VECTOR_BUILDER_FREEZE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "builder_freeze",
            "rt_arr__builder_freeze",
            vec![ref_array_null()],
            vec![ref_pvec_null()],
        )),
        id if id == prelude_ids::VECTOR_BUILDER_FROM => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "builder_from",
            "rt_arr__builder_from",
            vec![ref_pvec_null()],
            vec![ref_array_null()],
        )),
        id if id == prelude_ids::VECTOR_BUILDER_EXTEND => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.arr",
            "builder_extend",
            "rt_arr__builder_extend",
            vec![ref_array_null(), ref_pvec_null()],
            vec![],
        )),
        id if id == prelude_ids::STRING_BUILDER_FROM => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "builder_from",
            "rt_str__builder_from",
            vec![ref_string_null()],
            vec![ref_str_builder()],
        )),
        id if id == prelude_ids::STRING_BUILDER_EXTEND => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "builder_extend",
            "rt_str__builder_extend",
            vec![ref_str_builder_null(), ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::STRING_BUILDER_FREEZE => Some(PreludeEntry::runtime(
            twinkle_name,
            "rt.str",
            "builder_freeze",
            "rt_str__builder_freeze",
            vec![ref_str_builder_null()],
            vec![ref_string()],
        )),
        id if id == prelude_ids::HOST_READ_FILE => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "read_file",
            "host_read_file",
            vec![ref_string_null()],
            vec![ValType::Ref {
                nullable: true,
                heap: HeapType::Named(T_VARIANT.into()),
            }],
        )),
        id if id == prelude_ids::HOST_WRITE_FILE => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "write_file",
            "host_write_file",
            vec![ref_string_null(), ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::HOST_WRITE_BYTES => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "write_bytes",
            "host_write_bytes",
            vec![ref_string_null(), ref_array_null()],
            vec![],
        )),
        id if id == prelude_ids::HOST_MKDIRP => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "mkdirp",
            "host_mkdirp",
            vec![ref_string_null()],
            vec![],
        )),
        id if id == prelude_ids::HOST_LIST_DIR => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "list_dir",
            "host_list_dir",
            vec![ref_string_null()],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::HOST_EXISTS => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "exists",
            "host_exists",
            vec![ref_string_null()],
            vec![ValType::I32],
        )),
        id if id == prelude_ids::HOST_ARGS => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "args",
            "host_args",
            vec![],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::HOST_ENV => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "env",
            "host_env",
            vec![ref_string_null()],
            vec![ref_pvec()],
        )),
        id if id == prelude_ids::HOST_CWD => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "cwd",
            "host_cwd",
            vec![],
            vec![ref_string()],
        )),
        id if id == prelude_ids::HOST_EXIT => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "exit",
            "host_exit",
            vec![ValType::I64],
            vec![],
        )),
        id if id == prelude_ids::HOST_NOW => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "now",
            "host_now",
            vec![],
            vec![ValType::F64],
        )),
        id if id == prelude_ids::HOST_RUN_WASM => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "run_wasm",
            "host_run_wasm",
            vec![ref_array_null(), ref_array_null()],
            vec![ValType::I64],
        )),
        id if id == prelude_ids::HOST_STDIN_READ_CHUNK => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "stdin_read_chunk",
            "host_stdin_read_chunk",
            vec![ValType::I32],
            vec![ref_array()],
        )),
        id if id == prelude_ids::HOST_STDIN_READ_TIMEOUT => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "stdin_read_timeout",
            "host_stdin_read_timeout",
            vec![ValType::I32, ValType::I32],
            vec![ref_array()],
        )),
        id if id == prelude_ids::HOST_STDIN_EOF => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "stdin_eof",
            "host_stdin_eof",
            vec![],
            vec![ValType::I32],
        )),
        id if id == prelude_ids::HOST_STDOUT_WRITE_BYTES => Some(PreludeEntry::runtime(
            twinkle_name,
            "host",
            "stdout_write_bytes",
            "host_stdout_write_bytes",
            vec![ref_array_null()],
            vec![],
        )),
        _ => None,
    }
}

pub fn build_prelude_map() -> PreludeMap {
    let mut map = HashMap::new();

    for spec in registry::all_specs() {
        let entry = match spec.dispatch {
            IntrinsicDispatch::Intrinsic => PreludeEntry::intrinsic(spec.twinkle_name),
            IntrinsicDispatch::Runtime => runtime_entry(spec.func_id, spec.twinkle_name)
                .unwrap_or_else(|| {
                    panic!(
                        "missing runtime prelude binding for FuncId({}) '{}'",
                        spec.func_id.0, spec.twinkle_name
                    )
                }),
            IntrinsicDispatch::LibraryInternal => {
                // No library-internal intrinsics currently registered.
                continue;
            }
        };
        map.insert(spec.func_id, entry);
    }

    debug_assert!(map.contains_key(&prelude_ids::HOST_EXIT));
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intrinsics::registry::{self, IntrinsicDispatch};

    #[test]
    fn prelude_map_covers_all_fixed_ids() {
        let map = build_prelude_map();
        for id in prelude_ids::fixed_prelude_id_range() {
            if prelude_ids::is_retired_prelude_id(FuncId(id)) {
                continue;
            }
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
            prelude_ids::HOST_NOW.0,
            prelude_ids::HOST_RUN_WASM.0,
            prelude_ids::HOST_STDIN_READ_CHUNK.0,
            prelude_ids::HOST_STDIN_READ_TIMEOUT.0,
            prelude_ids::HOST_STDIN_EOF.0,
            prelude_ids::HOST_STDOUT_WRITE_BYTES.0,
            prelude_ids::CHAR_CODE_AT.0,
            prelude_ids::FROM_CHAR_CODE.0,
            prelude_ids::FROM_BYTE.0,
            prelude_ids::STRING_UTF8_BYTES.0,
            prelude_ids::STRING_FROM_UTF8.0,
            prelude_ids::FLOAT_BITS.0,
            prelude_ids::INT_FROM_STRING.0,
            prelude_ids::FLOAT_FROM_STRING.0,
        ] {
            assert!(
                map.contains_key(&FuncId(id)),
                "missing host prelude FuncId({id})"
            );
        }

        for retired in prelude_ids::RETIRED_PRELUDE_IDS {
            assert!(
                !map.contains_key(&retired.func_id),
                "retired prelude FuncId({}) should not exist in prelude map",
                retired.func_id.0
            );
            if let Some(replacement) = retired.replacement {
                assert!(
                    map.contains_key(&replacement),
                    "replacement FuncId({}) for retired FuncId({}) missing",
                    replacement.0,
                    retired.func_id.0
                );
            }
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

    #[test]
    fn prelude_map_matches_canonical_registry() {
        let map = build_prelude_map();
        assert_eq!(map.len(), registry::all_specs().len());

        for spec in registry::all_specs() {
            let entry = map
                .get(&spec.func_id)
                .unwrap_or_else(|| panic!("missing prelude entry for FuncId({})", spec.func_id.0));
            assert_eq!(entry.twinkle_name, spec.twinkle_name);
            assert_eq!(
                entry.is_runtime_call(),
                matches!(
                    spec.dispatch,
                    IntrinsicDispatch::Runtime | IntrinsicDispatch::LibraryInternal
                ),
                "dispatch mismatch for FuncId({})",
                spec.func_id.0
            );
        }
    }
}
