use std::sync::OnceLock;

use crate::ir::FuncId;
use crate::ir::lower::prelude as prelude_ids;

/// Classification for how a builtin is dispatched in stage0 codegen.
///
/// Each intrinsic falls into one of three categories:
///
/// - **Runtime**: forwarded as a Wasm import call to the JS/host runtime.
///   Used for I/O, string conversion, and data-structure operations that
///   require complex host logic (e.g. persistent vector/dict internals).
///
/// - **Intrinsic**: lowered to inline Wasm instructions via a `LoweringKind`.
///   Used when the operation maps naturally to Wasm primitives (comparisons,
///   struct/array access, identity coercions) and the runtime overhead of an
///   import call is unnecessary. Each instruction-level intrinsic must maintain
///   semantic parity with its corresponding `prelude/*.tw` implementation.
///
/// - **LibraryInternal**: like Runtime but not user-visible through the prelude.
///   Available only to `boot/lib` modules for internal compiler plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicDispatch {
    Runtime,
    Intrinsic,
    /// Library-internal builtins: not exposed through the prelude, but
    /// available to `boot/lib` modules via `populate_func_table`.  Each
    /// entry maps directly to a runtime substrate import (e.g. `rt.arr`).
    LibraryInternal,
}

/// Instruction-level lowering kinds for intrinsic dispatch.
///
/// Each variant here maps to a dedicated codegen function in `src/codegen/emit.rs`.
/// The rationale for instruction-level lowering (rather than a runtime import or
/// compiling the prelude `.tw` source) falls into a few categories:
///
/// - **Identity/coercion**: no-op at runtime, just satisfies the type system
///   (e.g. `StringToStringIdentity`).
/// - **GC struct/array access**: requires direct Wasm GC instructions that cannot
///   be expressed in Twinkle source (e.g. `CellGet`, `VectorGet`, `DictGet`).
/// - **Primitive comparison**: maps to Wasm comparison instructions for performance;
///   must match NaN/overflow semantics of the prelude `.tw` implementation.
/// - **Constructor lowering**: builds Wasm GC structs inline rather than calling
///   through the runtime (e.g. `CellNew`, `Range`, `IteratorUnfold`).
/// - **Encoding/conversion**: uses Wasm i32/i64 instructions directly for byte/int
///   coercions that are trivial at the instruction level (e.g. `ByteToInt`).
/// - **Task operations**: lowered to Wasm instructions for the async runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoweringKind {
    /// String.to_string is identity — no runtime call needed.
    StringToStringIdentity,
    /// Vector.append: GC array tail-push via struct access.
    VectorPush,
    /// range(n): constructs Range struct inline.
    Range,
    /// range_from(start, end): constructs Range struct inline.
    RangeFrom,
    /// range_step(start, end, step): constructs Range struct inline.
    RangeStep,
    /// Cell.new: allocates GC struct with initial value.
    CellNew,
    /// Cell.get: reads field 0 from GC struct.
    CellGet,
    /// Cell.set: writes field 0 of GC struct.
    CellSet,
    /// Cell.update: reads, applies function, writes back.
    CellUpdate,
    /// Dict.get: GC struct access + HAMT lookup.
    DictGet,
    /// dict_get_unsafe: like DictGet but traps on missing key.
    DictGetUnsafe,
    /// Iterator.unfold: constructs Iterator GC struct from seed + step fn.
    IteratorUnfold,
    /// Iterator.next: advances iterator via GC struct field access.
    IteratorNext,
    /// Vector.make: allocates GC array with repeated value.
    VectorMake,
    /// Vector.get: bounds-checked GC array access returning Option.
    VectorGet,
    /// Vector.set: bounds-checked GC array copy-on-write returning Option.
    VectorSet,
    /// __vector_set_in_place: mutable GC array write (internal).
    VectorSetInPlace,
    /// String.get: byte-level GC array access returning Option<Byte>.
    StringGet,
    /// String.slice: byte-range extraction from GC string.
    StringSlice,
    /// String.char_code_at: i32 byte extraction from GC string.
    CharCodeAt,
    /// String.from_char_code: creates single-char string from code point.
    FromCharCode,
    /// String.from_byte: creates single-byte string.
    FromByte,
    /// String.from_code_point: creates string from Unicode code point.
    FromCodePoint,
    /// String.utf8_bytes: copies string bytes to GC array.
    StringUtf8Bytes,
    /// String.from_utf8: validates and creates string from byte array.
    StringFromUtf8,
    /// Int.from_string: runtime-assisted parse with i64 result.
    IntFromString,
    /// Float.from_string: runtime-assisted parse with f64 result.
    FloatFromString,
    /// Byte.to_int: i32 → i64 extend (trivial instruction).
    ByteToInt,
    /// Byte.from_int: i64 → i32 wrap with range check.
    ByteFromInt,
    /// Byte.to_string: converts byte to its decimal string representation.
    ByteToString,
    /// Float.bits: f64 → i64 reinterpret (single Wasm instruction).
    FloatBits,
    /// Int.compare: i64 signed comparison → Order variant.
    IntCompare,
    /// Float.compare: f64 comparison with NaN handling → Order variant.
    /// NaN semantics match prelude/float.tw: NaN > non-NaN, NaN == NaN.
    FloatCompare,
    /// String.compare: delegates to runtime rt_str__cmp, wraps result as Order.
    StringCompare,
    /// Byte.compare: i32 unsigned comparison → Order variant.
    ByteCompare,
    /// Task.spawn: constructs task GC struct with thunk.
    TaskSpawn,
    /// Task.await: blocks on task completion, extracts result.
    TaskAwait,
    /// Task.yield: yields execution (currently a no-op stub).
    TaskYield,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrinsicSpec {
    pub func_id: FuncId,
    pub twinkle_name: &'static str,
    pub dispatch: IntrinsicDispatch,
    pub include_in_signature_registry: bool,
    pub include_in_contract_registry: bool,
    /// Instruction-level lowering kind for intrinsic dispatch, or `None` for
    /// runtime-forwarded calls that don't need special lowering.
    pub lowering_kind: Option<LoweringKind>,
}

macro_rules! spec {
    ($id:ident, $name:literal, $dispatch:ident, $sig:expr, $contract:expr) => {
        IntrinsicSpec {
            func_id: prelude_ids::$id,
            twinkle_name: $name,
            dispatch: IntrinsicDispatch::$dispatch,
            include_in_signature_registry: $sig,
            include_in_contract_registry: $contract,
            lowering_kind: None,
        }
    };
    ($id:ident, $name:literal, $dispatch:ident, $sig:expr, $contract:expr, $kind:ident) => {
        IntrinsicSpec {
            func_id: prelude_ids::$id,
            twinkle_name: $name,
            dispatch: IntrinsicDispatch::$dispatch,
            include_in_signature_registry: $sig,
            include_in_contract_registry: $contract,
            lowering_kind: Some(LoweringKind::$kind),
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
    spec!(
        STRING_TO_STRING,
        "String.to_string",
        Intrinsic,
        true,
        true,
        StringToStringIdentity
    ),
    spec!(STRING_LEN, "String.len", Runtime, true, true),
    spec!(STRING_CONCAT, "String.concat", Runtime, true, true),
    spec!(
        STRING_COMPARE,
        "String.compare",
        Intrinsic,
        true,
        true,
        StringCompare
    ),
    spec!(STRING_GET, "String.get", Intrinsic, true, true, StringGet),
    spec!(
        STRING_SLICE,
        "String.slice",
        Intrinsic,
        true,
        true,
        StringSlice
    ),
    spec!(VECTOR_LEN, "Vector.len", Runtime, true, true),
    spec!(
        VECTOR_APPEND,
        "Vector.append",
        Intrinsic,
        true,
        true,
        VectorPush
    ),
    spec!(
        VECTOR_SET_UNSAFE,
        "Vector.set_unsafe",
        Runtime,
        false,
        false
    ),
    spec!(VECTOR_CONCAT, "Vector.concat", Runtime, true, true),
    spec!(VECTOR_SLICE, "Vector.slice", Runtime, true, true),
    spec!(VECTOR_DROP_LAST, "Vector.drop_last", Runtime, true, true),
    spec!(DICT_SET, "Dict.set", Runtime, true, true),
    spec!(DICT_KEYS, "Dict.keys", Runtime, true, true),
    spec!(DICT_GET, "Dict.get", Intrinsic, true, true, DictGet),
    spec!(DICT_NEW, "Dict.new", Runtime, true, true),
    spec!(DICT_LEN, "Dict.len", Runtime, true, true),
    spec!(DICT_HAS, "Dict.has", Runtime, true, true),
    spec!(DICT_REMOVE, "Dict.remove", Runtime, true, true),
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
    spec!(RANGE_FROM, "range_from", Intrinsic, true, true, RangeFrom),
    spec!(RANGE, "range", Intrinsic, true, true, Range),
    spec!(RANGE_STEP, "range_step", Intrinsic, true, true, RangeStep),
    spec!(CELL_NEW, "Cell.new", Intrinsic, true, true, CellNew),
    spec!(CELL_GET, "Cell.get", Intrinsic, true, true, CellGet),
    spec!(CELL_SET, "Cell.set", Intrinsic, true, true, CellSet),
    spec!(
        CELL_UPDATE,
        "Cell.update",
        Intrinsic,
        true,
        true,
        CellUpdate
    ),
    spec!(
        DICT_GET_UNSAFE,
        "dict_get_unsafe",
        Intrinsic,
        false,
        true,
        DictGetUnsafe
    ),
    spec!(
        ITERATOR_NEXT,
        "Iterator.next",
        Intrinsic,
        true,
        true,
        IteratorNext
    ),
    spec!(
        ITERATOR_UNFOLD,
        "Iterator.unfold",
        Intrinsic,
        true,
        true,
        IteratorUnfold
    ),
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
    spec!(VECTOR_GET, "Vector.get", Intrinsic, true, true, VectorGet),
    spec!(VECTOR_SET, "Vector.set", Intrinsic, true, true, VectorSet),
    spec!(
        VECTOR_MAKE,
        "Vector.make",
        Intrinsic,
        true,
        true,
        VectorMake
    ),
    spec!(
        VECTOR_SET_IN_PLACE,
        "__vector_set_in_place",
        Intrinsic,
        false,
        true,
        VectorSetInPlace
    ),
    spec!(
        VECTOR_BUILDER_FROM,
        "__vector_builder_from",
        Runtime,
        false,
        false
    ),
    spec!(
        VECTOR_BUILDER_EXTEND,
        "__vector_builder_extend",
        Runtime,
        false,
        false
    ),
    spec!(
        STRING_BUILDER_FROM,
        "__string_builder_from",
        Runtime,
        false,
        false
    ),
    spec!(
        STRING_BUILDER_EXTEND,
        "__string_builder_extend",
        Runtime,
        false,
        false
    ),
    spec!(
        STRING_BUILDER_FREEZE,
        "__string_builder_freeze",
        Runtime,
        false,
        false
    ),
    spec!(BYTE_TO_INT, "Byte.to_int", Intrinsic, true, true, ByteToInt),
    spec!(
        BYTE_FROM_INT,
        "Byte.from_int",
        Intrinsic,
        true,
        true,
        ByteFromInt
    ),
    spec!(
        BYTE_TO_STRING,
        "Byte.to_string",
        Intrinsic,
        true,
        true,
        ByteToString
    ),
    spec!(
        CHAR_CODE_AT,
        "String.char_code_at",
        Intrinsic,
        true,
        true,
        CharCodeAt
    ),
    spec!(
        FROM_CHAR_CODE,
        "String.from_char_code",
        Intrinsic,
        true,
        true,
        FromCharCode
    ),
    spec!(
        FROM_BYTE,
        "String.from_byte",
        Intrinsic,
        true,
        true,
        FromByte
    ),
    spec!(
        FROM_CODE_POINT,
        "String.from_code_point",
        Intrinsic,
        true,
        true,
        FromCodePoint
    ),
    spec!(
        STRING_UTF8_BYTES,
        "String.utf8_bytes",
        Intrinsic,
        true,
        true,
        StringUtf8Bytes
    ),
    spec!(
        STRING_FROM_UTF8,
        "String.from_utf8",
        Intrinsic,
        true,
        true,
        StringFromUtf8
    ),
    spec!(FLOAT_BITS, "Float.bits", Intrinsic, true, true, FloatBits),
    spec!(
        INT_COMPARE,
        "Int.compare",
        Intrinsic,
        true,
        true,
        IntCompare
    ),
    spec!(
        FLOAT_COMPARE,
        "Float.compare",
        Intrinsic,
        true,
        true,
        FloatCompare
    ),
    spec!(
        BYTE_COMPARE,
        "Byte.compare",
        Intrinsic,
        true,
        true,
        ByteCompare
    ),
    spec!(
        INT_FROM_STRING,
        "Int.from_string",
        Intrinsic,
        true,
        true,
        IntFromString
    ),
    spec!(
        FLOAT_FROM_STRING,
        "Float.from_string",
        Intrinsic,
        true,
        true,
        FloatFromString
    ),
    spec!(TASK_SPAWN, "Task.spawn", Intrinsic, true, true, TaskSpawn),
    spec!(TASK_AWAIT, "Task.await", Intrinsic, true, true, TaskAwait),
    spec!(TASK_YIELD, "Task.yield", Intrinsic, true, true, TaskYield),
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
    spec!(HOST_NOW, "__host_now", Runtime, false, false),
    spec!(HOST_RUN_WASM, "__host_run_wasm", Runtime, false, false),
    spec!(
        HOST_STDIN_READ_CHUNK,
        "__host_stdin_read_chunk",
        Runtime,
        false,
        false
    ),
    spec!(
        HOST_STDIN_READ_TIMEOUT,
        "__host_stdin_read_timeout",
        Runtime,
        false,
        false
    ),
    spec!(HOST_STDIN_EOF, "__host_stdin_eof", Runtime, false, false),
    spec!(
        HOST_STDOUT_WRITE_BYTES,
        "__host_stdout_write_bytes",
        Runtime,
        false,
        false
    ),
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
    ("Vector.drop_last", prelude_ids::VECTOR_DROP_LAST),
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
    ("__host_now", prelude_ids::HOST_NOW),
    ("__host_run_wasm", prelude_ids::HOST_RUN_WASM),
    (
        "__host_stdin_read_chunk",
        prelude_ids::HOST_STDIN_READ_CHUNK,
    ),
    (
        "__host_stdin_read_timeout",
        prelude_ids::HOST_STDIN_READ_TIMEOUT,
    ),
    ("__host_stdin_eof", prelude_ids::HOST_STDIN_EOF),
    (
        "__host_stdout_write_bytes",
        prelude_ids::HOST_STDOUT_WRITE_BYTES,
    ),
];

const LEGACY_BOOTSTRAP_FUNC_NAMES: &[(&str, FuncId)] = &[
    ("string_len", prelude_ids::STRING_LEN),
    ("string_concat", prelude_ids::STRING_CONCAT),
    ("vector_len", prelude_ids::VECTOR_LEN),
    ("vector_append", prelude_ids::VECTOR_APPEND),
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
    // Library-internal builtins are always registered so boot/lib
    // modules can reference them by name.
    for spec in all_specs()
        .iter()
        .filter(|spec| spec.dispatch == IntrinsicDispatch::LibraryInternal)
    {
        func_table.insert(spec.twinkle_name.to_string(), spec.func_id);
    }
}

pub fn builtin_module_aliases() -> &'static [&'static str] {
    &[
        "Cell", "Dict", "Iterator", "Option", "Result", "Set", "Task", "Vector", "String", "Int",
        "Float", "Bool", "Byte",
    ]
}

/// Look up the lowering kind for an intrinsic by func_id.
/// Uses the unified spec table instead of a separate match block.
pub fn lowering_kind(func_id: FuncId) -> Option<LoweringKind> {
    spec(func_id).and_then(|s| s.lowering_kind)
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
                IntrinsicDispatch::Runtime | IntrinsicDispatch::LibraryInternal => assert!(
                    lowering.is_none(),
                    "runtime/library-internal FuncId({}) should not have intrinsic lowering kind",
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
