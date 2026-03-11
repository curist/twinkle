use std::sync::OnceLock;

use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicDispatch {
    Runtime,
    Intrinsic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweringKind {
    StringToStringIdentity,
    VectorPush,
    Range,
    RangeFrom,
    RangeStep,
    CellNew,
    CellGet,
    CellSet,
    CellUpdate,
    DictGetUnsafe,
    IteratorUnfold,
    IteratorNext,
    VectorMake,
    VectorGet,
    VectorSet,
    VectorSetInPlace,
    StringGet,
    StringSlice,
    CharCodeAt,
    FromCharCode,
    FromCodePoint,
    StringUtf8Bytes,
    StringFromUtf8,
    IntFromString,
    FloatFromString,
    ByteToInt,
    ByteFromInt,
    ByteToString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrinsicSpec {
    pub func_id: FuncId,
    pub twinkle_name: &'static str,
    pub dispatch: IntrinsicDispatch,
    pub include_in_signature_registry: bool,
    pub include_in_contract_registry: bool,
}

macro_rules! spec {
    ($id:ident, $name:literal, $dispatch:ident, $sig:expr, $contract:expr) => {
        IntrinsicSpec {
            func_id: prelude_ids::$id,
            twinkle_name: $name,
            dispatch: IntrinsicDispatch::$dispatch,
            include_in_signature_registry: $sig,
            include_in_contract_registry: $contract,
        }
    };
}

const INTRINSIC_SPECS: &[IntrinsicSpec] = &[
    spec!(PRINT, "print", Runtime, false, false),
    spec!(PRINTLN, "println", Runtime, false, false),
    spec!(ERROR, "error", Runtime, false, false),
    spec!(EPRINT, "eprint", Runtime, false, false),
    spec!(EPRINTLN, "eprintln", Runtime, false, false),
    spec!(INT_TO_STRING, "Int.to_string", Runtime, true, true),
    spec!(FLOAT_TO_STRING, "Float.to_string", Runtime, true, true),
    spec!(BOOL_TO_STRING, "Bool.to_string", Runtime, true, true),
    spec!(STRING_TO_STRING, "String.to_string", Intrinsic, true, true),
    spec!(STRING_LEN, "String.len", Runtime, false, false),
    spec!(STRING_CONCAT, "String.concat", Runtime, false, false),
    spec!(STRING_GET, "String.get", Intrinsic, true, true),
    spec!(STRING_SLICE, "String.slice", Intrinsic, true, true),
    spec!(VECTOR_LEN, "Vector.len", Runtime, false, false),
    spec!(VECTOR_PUSH, "Vector.push", Intrinsic, true, true),
    spec!(
        VECTOR_SET_UNSAFE,
        "Vector.set_unsafe",
        Runtime,
        false,
        false
    ),
    spec!(VECTOR_CONCAT, "Vector.concat", Runtime, false, false),
    spec!(VECTOR_SLICE, "Vector.slice", Runtime, false, false),
    spec!(DICT_SET, "Dict.set", Runtime, false, false),
    spec!(DICT_KEYS, "Dict.keys", Runtime, false, false),
    spec!(DICT_GET, "dict_get", Runtime, false, false),
    spec!(DICT_NEW, "Dict.new", Runtime, false, false),
    spec!(DICT_LEN, "Dict.len", Runtime, false, false),
    spec!(DICT_HAS, "Dict.has", Runtime, false, false),
    spec!(DICT_REMOVE, "Dict.remove", Runtime, false, false),
    spec!(
        DICT_SET_IN_PLACE,
        "__dict_set_in_place",
        Runtime,
        false,
        false
    ),
    spec!(
        DICT_REMOVE_IN_PLACE,
        "__dict_remove_in_place",
        Runtime,
        false,
        false
    ),
    spec!(RANGE_FROM, "range_from", Intrinsic, true, true),
    spec!(RANGE, "range", Intrinsic, true, true),
    spec!(RANGE_STEP, "range_step", Intrinsic, true, true),
    spec!(CELL_NEW, "Cell.new", Intrinsic, true, true),
    spec!(CELL_GET, "Cell.get", Intrinsic, true, true),
    spec!(CELL_SET, "Cell.set", Intrinsic, true, true),
    spec!(CELL_UPDATE, "Cell.update", Intrinsic, true, true),
    spec!(DICT_GET_UNSAFE, "dict_get_unsafe", Intrinsic, false, true),
    spec!(ITERATOR_NEXT, "Iterator.next", Intrinsic, true, true),
    spec!(ITERATOR_UNFOLD, "Iterator.unfold", Intrinsic, true, true),
    spec!(
        VECTOR_BUILDER_NEW,
        "__vector_builder_new",
        Runtime,
        false,
        false
    ),
    spec!(
        VECTOR_BUILDER_PUSH,
        "__vector_builder_push",
        Runtime,
        false,
        false
    ),
    spec!(
        VECTOR_BUILDER_FREEZE,
        "__vector_builder_freeze",
        Runtime,
        false,
        false
    ),
    spec!(VECTOR_GET, "Vector.get", Intrinsic, true, true),
    spec!(VECTOR_SET, "Vector.set", Intrinsic, true, true),
    spec!(VECTOR_MAKE, "Vector.make", Intrinsic, true, true),
    spec!(
        VECTOR_SET_IN_PLACE,
        "__vector_set_in_place",
        Intrinsic,
        false,
        true
    ),
    spec!(
        VECTOR_BUILDER_FROM,
        "__vector_builder_from",
        Runtime,
        false,
        false
    ),
    spec!(BYTE_TO_INT, "Byte.to_int", Intrinsic, true, true),
    spec!(BYTE_FROM_INT, "Byte.from_int", Intrinsic, true, true),
    spec!(BYTE_TO_STRING, "Byte.to_string", Intrinsic, true, true),
    spec!(CHAR_CODE_AT, "String.char_code_at", Intrinsic, true, true),
    spec!(
        FROM_CHAR_CODE,
        "String.from_char_code",
        Intrinsic,
        true,
        true
    ),
    spec!(
        FROM_CODE_POINT,
        "String.from_code_point",
        Intrinsic,
        true,
        true
    ),
    spec!(
        STRING_UTF8_BYTES,
        "String.utf8_bytes",
        Intrinsic,
        true,
        true
    ),
    spec!(STRING_FROM_UTF8, "String.from_utf8", Intrinsic, true, true),
    spec!(INT_FROM_STRING, "Int.from_string", Intrinsic, true, true),
    spec!(
        FLOAT_FROM_STRING,
        "Float.from_string",
        Intrinsic,
        true,
        true
    ),
    spec!(HOST_READ_FILE, "__host_read_file", Runtime, false, false),
    spec!(HOST_WRITE_FILE, "__host_write_file", Runtime, false, false),
    spec!(
        HOST_WRITE_BYTES,
        "__host_write_bytes",
        Runtime,
        false,
        false
    ),
    spec!(HOST_MKDIRP, "__host_mkdirp", Runtime, false, false),
    spec!(HOST_LIST_DIR, "__host_list_dir", Runtime, false, false),
    spec!(HOST_EXISTS, "__host_exists", Runtime, false, false),
    spec!(HOST_ARGS, "__host_args", Runtime, false, false),
    spec!(HOST_ENV, "__host_env", Runtime, false, false),
    spec!(HOST_CWD, "__host_cwd", Runtime, false, false),
    spec!(HOST_EXIT, "__host_exit", Runtime, false, false),
];

pub fn all_specs() -> &'static [IntrinsicSpec] {
    INTRINSIC_SPECS
}

pub fn spec(func_id: FuncId) -> Option<&'static IntrinsicSpec> {
    INTRINSIC_SPECS.iter().find(|spec| spec.func_id == func_id)
}

pub fn signature_func_ids() -> &'static [FuncId] {
    static IDS: OnceLock<Vec<FuncId>> = OnceLock::new();
    IDS.get_or_init(|| {
        INTRINSIC_SPECS
            .iter()
            .filter(|spec| spec.include_in_signature_registry)
            .map(|spec| spec.func_id)
            .collect()
    })
    .as_slice()
}

pub fn contract_func_ids() -> &'static [FuncId] {
    static IDS: OnceLock<Vec<FuncId>> = OnceLock::new();
    IDS.get_or_init(|| {
        INTRINSIC_SPECS
            .iter()
            .filter(|spec| spec.include_in_contract_registry)
            .map(|spec| spec.func_id)
            .collect()
    })
    .as_slice()
}

const COMMON_BOOTSTRAP_FUNC_NAMES: &[(&str, FuncId)] = &[
    ("print", prelude_ids::PRINT),
    ("println", prelude_ids::PRINTLN),
    ("error", prelude_ids::ERROR),
    ("eprint", prelude_ids::EPRINT),
    ("eprintln", prelude_ids::EPRINTLN),
    ("Dict.new", prelude_ids::DICT_NEW),
    ("Vector.len", prelude_ids::VECTOR_LEN),
    ("Vector.concat", prelude_ids::VECTOR_CONCAT),
    ("Vector.slice", prelude_ids::VECTOR_SLICE),
    ("String.len", prelude_ids::STRING_LEN),
    ("String.concat", prelude_ids::STRING_CONCAT),
    ("Dict.len", prelude_ids::DICT_LEN),
    ("Dict.has", prelude_ids::DICT_HAS),
    ("Dict.keys", prelude_ids::DICT_KEYS),
    ("Dict.remove", prelude_ids::DICT_REMOVE),
    ("__host_read_file", prelude_ids::HOST_READ_FILE),
    ("__host_write_file", prelude_ids::HOST_WRITE_FILE),
    ("__host_write_bytes", prelude_ids::HOST_WRITE_BYTES),
    ("__host_mkdirp", prelude_ids::HOST_MKDIRP),
    ("__host_list_dir", prelude_ids::HOST_LIST_DIR),
    ("__host_exists", prelude_ids::HOST_EXISTS),
    ("__host_args", prelude_ids::HOST_ARGS),
    ("__host_env", prelude_ids::HOST_ENV),
    ("__host_cwd", prelude_ids::HOST_CWD),
    ("__host_exit", prelude_ids::HOST_EXIT),
];

const LEGACY_BOOTSTRAP_FUNC_NAMES: &[(&str, FuncId)] = &[
    ("string_len", prelude_ids::STRING_LEN),
    ("string_concat", prelude_ids::STRING_CONCAT),
    ("vector_len", prelude_ids::VECTOR_LEN),
    ("vector_push", prelude_ids::VECTOR_PUSH),
    ("vector_set_unsafe", prelude_ids::VECTOR_SET_UNSAFE),
    ("dict_set", prelude_ids::DICT_SET),
    ("dict_keys", prelude_ids::DICT_KEYS),
];

pub fn populate_func_table(
    func_table: &mut std::collections::HashMap<String, FuncId>,
    include_legacy_aliases: bool,
) {
    for (name, func_id) in COMMON_BOOTSTRAP_FUNC_NAMES {
        func_table.insert((*name).to_string(), *func_id);
    }
    if include_legacy_aliases {
        for (name, func_id) in LEGACY_BOOTSTRAP_FUNC_NAMES {
            func_table.insert((*name).to_string(), *func_id);
        }
    }
    for spec in all_specs()
        .iter()
        .filter(|spec| spec.include_in_signature_registry)
    {
        func_table.insert(spec.twinkle_name.to_string(), spec.func_id);
    }
}

pub fn builtin_module_aliases() -> &'static [&'static str] {
    &[
        "Cell", "Dict", "Iterator", "Vector", "String", "Int", "Float", "Bool", "Byte",
    ]
}

pub fn lowering_kind(func_id: FuncId) -> Option<LoweringKind> {
    match func_id {
        id if id == prelude_ids::STRING_TO_STRING => Some(LoweringKind::StringToStringIdentity),
        id if id == prelude_ids::VECTOR_PUSH => Some(LoweringKind::VectorPush),
        id if id == prelude_ids::RANGE => Some(LoweringKind::Range),
        id if id == prelude_ids::RANGE_FROM => Some(LoweringKind::RangeFrom),
        id if id == prelude_ids::RANGE_STEP => Some(LoweringKind::RangeStep),
        id if id == prelude_ids::CELL_NEW => Some(LoweringKind::CellNew),
        id if id == prelude_ids::CELL_GET => Some(LoweringKind::CellGet),
        id if id == prelude_ids::CELL_SET => Some(LoweringKind::CellSet),
        id if id == prelude_ids::CELL_UPDATE => Some(LoweringKind::CellUpdate),
        id if id == prelude_ids::DICT_GET_UNSAFE => Some(LoweringKind::DictGetUnsafe),
        id if id == prelude_ids::ITERATOR_UNFOLD => Some(LoweringKind::IteratorUnfold),
        id if id == prelude_ids::ITERATOR_NEXT => Some(LoweringKind::IteratorNext),
        id if id == prelude_ids::VECTOR_MAKE => Some(LoweringKind::VectorMake),
        id if id == prelude_ids::VECTOR_GET => Some(LoweringKind::VectorGet),
        id if id == prelude_ids::VECTOR_SET => Some(LoweringKind::VectorSet),
        id if id == prelude_ids::VECTOR_SET_IN_PLACE => Some(LoweringKind::VectorSetInPlace),
        id if id == prelude_ids::STRING_GET => Some(LoweringKind::StringGet),
        id if id == prelude_ids::STRING_SLICE => Some(LoweringKind::StringSlice),
        id if id == prelude_ids::CHAR_CODE_AT => Some(LoweringKind::CharCodeAt),
        id if id == prelude_ids::FROM_CHAR_CODE => Some(LoweringKind::FromCharCode),
        id if id == prelude_ids::FROM_CODE_POINT => Some(LoweringKind::FromCodePoint),
        id if id == prelude_ids::STRING_UTF8_BYTES => Some(LoweringKind::StringUtf8Bytes),
        id if id == prelude_ids::STRING_FROM_UTF8 => Some(LoweringKind::StringFromUtf8),
        id if id == prelude_ids::INT_FROM_STRING => Some(LoweringKind::IntFromString),
        id if id == prelude_ids::FLOAT_FROM_STRING => Some(LoweringKind::FloatFromString),
        id if id == prelude_ids::BYTE_TO_INT => Some(LoweringKind::ByteToInt),
        id if id == prelude_ids::BYTE_FROM_INT => Some(LoweringKind::ByteFromInt),
        id if id == prelude_ids::BYTE_TO_STRING => Some(LoweringKind::ByteToString),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_specs_have_unique_names_and_ids() {
        let mut names = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for spec in all_specs() {
            assert!(names.insert(spec.twinkle_name));
            assert!(ids.insert(spec.func_id.0));
        }
    }

    #[test]
    fn contract_ids_are_not_empty() {
        assert!(!contract_func_ids().is_empty());
    }

    #[test]
    fn registry_excludes_retired_prelude_ids() {
        for retired in prelude_ids::RETIRED_PRELUDE_IDS {
            assert!(
                spec(retired.func_id).is_none(),
                "retired prelude FuncId({}) leaked into canonical registry",
                retired.func_id.0
            );
            assert!(
                !all_specs()
                    .iter()
                    .any(|entry| entry.twinkle_name == retired.former_twinkle_name),
                "retired prelude name '{}' leaked into canonical registry",
                retired.former_twinkle_name
            );
            if let Some(replacement) = retired.replacement {
                assert!(
                    spec(replacement).is_some(),
                    "replacement FuncId({}) missing from canonical registry",
                    replacement.0
                );
            }
        }
    }

    #[test]
    fn lowering_kind_presence_matches_dispatch_kind() {
        for entry in all_specs() {
            let lowering = lowering_kind(entry.func_id);
            match entry.dispatch {
                IntrinsicDispatch::Runtime => assert!(
                    lowering.is_none(),
                    "runtime FuncId({}) should not have intrinsic lowering kind",
                    entry.func_id.0
                ),
                IntrinsicDispatch::Intrinsic => assert!(
                    lowering.is_some(),
                    "intrinsic FuncId({}) missing lowering kind",
                    entry.func_id.0
                ),
            }
        }
    }
}
