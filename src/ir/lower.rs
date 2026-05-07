use std::collections::{HashMap, HashSet};

use crate::intrinsics::registry;
use crate::syntax::ast::{
    BinOp, Block, CaseArm, Expr, ExprKind, ExternFunctionDecl, FunctionDecl, Item, Literal,
    Pattern, SourceFile, Stmt, StringPart,
};
use crate::syntax::span::Span;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::ty::{
    ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID, RANGE_TYPE_ID, RESULT_TYPE_ID,
    TypeId, method_receiver_type_id,
};
use crate::types::type_map::TypeMap;

use crate::module::artifacts::{ExternalFuncRef, LoweredModule};

use super::core::{
    CoreExpr, CoreExprKind, CoreModule, CorePattern, ExternImport, FieldId, FuncId, FunctionDef,
    LocalId, MatchArm, VariantId,
};
use super::error::LowerError;
use super::local_allocator::LocalAllocator;

// ---------------------------------------------------------------------------
// Prelude function IDs (fixed)
// ---------------------------------------------------------------------------

pub mod prelude {
    use super::FuncId;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RetiredPreludeId {
        pub func_id: FuncId,
        pub former_twinkle_name: &'static str,
        pub replacement: Option<FuncId>,
    }

    pub const PRINT: FuncId = FuncId(1);
    pub const PRINTLN: FuncId = FuncId(2);
    pub const ERROR: FuncId = FuncId(3);

    pub const INT_TO_STRING: FuncId = FuncId(4);
    pub const FLOAT_TO_STRING: FuncId = FuncId(5);
    pub const BOOL_TO_STRING: FuncId = FuncId(6);
    pub const STRING_TO_STRING: FuncId = FuncId(7); // identity

    pub const STRING_LEN: FuncId = FuncId(8);
    pub const STRING_CONCAT: FuncId = FuncId(9);

    pub const VECTOR_LEN: FuncId = FuncId(10);
    pub const VECTOR_APPEND: FuncId = FuncId(11);

    pub const VECTOR_SET_UNSAFE: FuncId = FuncId(12); // Vector.set_unsafe(vec, idx, val) -> Vector<T>
    pub const DICT_SET: FuncId = FuncId(13); // Dict.set(m, k, v) -> Dict<K,V>
    pub const DICT_KEYS: FuncId = FuncId(14); // Dict.keys(m) -> Vector<K>

    pub const RANGE_FROM: FuncId = FuncId(15); // range_from(start, end) -> Range
    pub const RANGE: FuncId = FuncId(16); // range(n) -> Range  (0..n)

    pub const CELL_NEW: FuncId = FuncId(17); // Cell.new(v: T) Cell<T>
    pub const CELL_GET: FuncId = FuncId(18); // Cell.get(c: Cell<T>) T
    pub const CELL_SET: FuncId = FuncId(19); // Cell.set(c: Cell<T>, v: T) Void
    pub const CELL_UPDATE: FuncId = FuncId(20); // Cell.update(c: Cell<T>, f: fn(T) T) Void

    pub const DICT_GET: FuncId = FuncId(21); // dict_get(m, k) -> Option<V>
    pub const DICT_NEW: FuncId = FuncId(22); // Dict.new() -> Dict<K,V>

    pub const RANGE_STEP: FuncId = FuncId(23); // range_step(start, end, step) -> Range

    pub const DICT_GET_UNSAFE: FuncId = FuncId(24); // internal: dict_get_unsafe(m, k) -> V  (panics if absent)

    pub const VECTOR_CONCAT: FuncId = FuncId(25); // Vector.concat(a, b) -> Vector<T>
    pub const VECTOR_SLICE: FuncId = FuncId(26); // Vector.slice(vec, start, end) -> Vector<T>
    pub const DICT_LEN: FuncId = FuncId(27); // Dict.len(m) -> Int
    pub const DICT_HAS: FuncId = FuncId(28); // Dict.has(m, k) -> Bool
    pub const DICT_REMOVE: FuncId = FuncId(29); // Dict.remove(m, k) -> Dict<K,V>
    pub const STRING_SUBSTR: FuncId = FuncId(30); // String.substring(s, start, end) -> String
    pub const STRING_GET: FuncId = FuncId(1021); // String.get(s, i) -> Option<String>

    pub const ITERATOR_NEXT: FuncId = FuncId(31); // Iterator.next<T>(it: Iterator<T>) Option<IterItem<T>>
    pub const ITERATOR_UNFOLD: FuncId = FuncId(32); // Iterator.unfold<T,S>(seed: S, step: fn(S) UnfoldStep<T,S>) Iterator<T>

    // Internal vector builder intrinsics (never user-visible)
    pub const VECTOR_BUILDER_NEW: FuncId = FuncId(33); // () -> Cell<Vector<T>>
    pub const VECTOR_BUILDER_PUSH: FuncId = FuncId(34); // (Cell<Vector<T>>, T) -> Void
    pub const VECTOR_BUILDER_FREEZE: FuncId = FuncId(35); // (Cell<Vector<T>>) -> Vector<T>

    pub const VECTOR_GET: FuncId = FuncId(38); // Vector.get(vec, i) -> Option<T>  (safe)
    pub const VECTOR_SET: FuncId = FuncId(39); // Vector.set(vec, i, val) -> Option<Vector<T>> (safe)
    pub const VECTOR_MAKE: FuncId = FuncId(40); // Vector.make(size: Int, fill: T) -> Vector<T>
    // Internal helper: mutate a preallocated vector slot and return the same vector.
    // Used by range-collect specialization to avoid O(N^2) append/concat growth.
    pub const VECTOR_SET_IN_PLACE: FuncId = FuncId(1013); // (vec, i, val) -> vec
    // Internal helper: create a vector builder seeded from an existing vector.
    // Used by uniqueness loop rewrite for non-empty accumulators.
    pub const VECTOR_BUILDER_FROM: FuncId = FuncId(1014); // (vec) -> Cell<Vector<T>>
    // Internal helper: uniqueness rewrite target for Dict.set on a unique base.
    pub const DICT_SET_IN_PLACE: FuncId = FuncId(1015); // (dict, key, val) -> Dict<K,V>
    // Internal helper: uniqueness rewrite target for Dict.remove on a unique base.
    pub const DICT_REMOVE_IN_PLACE: FuncId = FuncId(1016); // (dict, key) -> Dict<K,V>
    // Internal helper: extend a vector builder with all elements from a vector.
    pub const VECTOR_BUILDER_EXTEND: FuncId = FuncId(1100); // (builder, vec) -> Void

    // String / numeric conversion builtins
    pub const CHAR_CODE_AT: FuncId = FuncId(1017); // String.char_code_at(s, i) -> Int
    pub const FROM_CHAR_CODE: FuncId = FuncId(1018); // String.from_char_code(n) -> Option<String>
    pub const BYTE_TO_INT: FuncId = FuncId(1022); // Byte.to_int(b: Byte) -> Int
    pub const BYTE_FROM_INT: FuncId = FuncId(1023); // Byte.from_int(n: Int) -> Option<Byte>
    pub const BYTE_TO_STRING: FuncId = FuncId(1024); // Byte.to_string(b: Byte) -> String
    pub const STRING_SLICE: FuncId = FuncId(1025); // String.slice(s, start, end) -> String (UTF-8 boundary validated)
    pub const FROM_CODE_POINT: FuncId = FuncId(1026); // String.from_code_point(n: Int) -> Option<String>
    pub const STRING_UTF8_BYTES: FuncId = FuncId(1027); // String.utf8_bytes(s: String) -> Vector<Byte>
    pub const STRING_FROM_UTF8: FuncId = FuncId(1028); // String.from_utf8(bytes: Vector<Byte>) -> Option<String>
    pub const FLOAT_BITS: FuncId = FuncId(1029); // Float.bits(f: Float) -> Int  (IEEE 754 bit pattern as i64)

    pub const INT_FROM_STRING: FuncId = FuncId(1019); // (s: String) -> Option<Int>
    pub const FLOAT_FROM_STRING: FuncId = FuncId(1020); // (s: String) -> Option<Float>

    // Additional prelude functions (kept outside fixed 1..=40 range).
    pub const EPRINT: FuncId = FuncId(1007); // eprint(s: String) -> Void
    pub const EPRINTLN: FuncId = FuncId(1008); // eprintln(s: String) -> Void

    // Host stdlib bridge intrinsics used by `@std.fs` and `@std.proc`.
    // Kept outside the fixed 1..=40 prelude range so existing user FuncId
    // assignments (USER_FUNC_START=41) remain stable.
    pub const HOST_READ_FILE: FuncId = FuncId(1001); // (path: String) -> Result<Vector<Byte>, String>
    pub const HOST_WRITE_FILE: FuncId = FuncId(1002); // (path: String, content: String) -> Void
    pub const HOST_WRITE_BYTES: FuncId = FuncId(1003); // (path: String, bytes: Array<Int>) -> Void
    pub const HOST_MKDIRP: FuncId = FuncId(1004); // (path: String) -> Void
    pub const HOST_LIST_DIR: FuncId = FuncId(1005); // (path: String) -> Array<String>
    pub const HOST_EXISTS: FuncId = FuncId(1006); // (path: String) -> Bool
    pub const HOST_ARGS: FuncId = FuncId(1009); // () -> Array<String>
    pub const HOST_ENV: FuncId = FuncId(1010); // (name: String) -> Array<String> (0/1 values)
    pub const HOST_CWD: FuncId = FuncId(1011); // () -> String
    pub const HOST_EXIT: FuncId = FuncId(1012); // (code: Int) -> Never
    pub const HOST_NOW: FuncId = FuncId(1030); // () -> Float (milliseconds since time origin)
    pub const HOST_RUN_WASM: FuncId = FuncId(1031); // (bytes: Vector<Byte>, argv: Vector<String>) -> Int
    pub const HOST_STDIN_READ_CHUNK: FuncId = FuncId(1032); // (max_bytes: Int) -> Vector<Byte>
    pub const HOST_STDOUT_WRITE_BYTES: FuncId = FuncId(1033); // (bytes: Vector<Byte>) -> Void

    // User functions start here
    pub const USER_FUNC_START: u32 = 41;

    /// Fixed low prelude ID window reserved for long-lived compatibility.
    pub const FIXED_PRELUDE_ID_START: u32 = PRINT.0;
    pub const FIXED_PRELUDE_ID_END: u32 = VECTOR_BUILDER_FREEZE.0;

    /// Retired low-window IDs are intentionally never reused.
    /// Migration policy: keep the old ID reserved forever and document
    /// the replacement intrinsic ID used by new code.
    pub const RETIRED_PRELUDE_IDS: &[RetiredPreludeId] = &[RetiredPreludeId {
        func_id: STRING_SUBSTR,
        former_twinkle_name: "String.substring",
        replacement: Some(STRING_SLICE),
    }];

    pub fn fixed_prelude_id_range() -> std::ops::RangeInclusive<u32> {
        FIXED_PRELUDE_ID_START..=FIXED_PRELUDE_ID_END
    }

    pub fn retired_prelude_id(func_id: FuncId) -> Option<&'static RetiredPreludeId> {
        RETIRED_PRELUDE_IDS
            .iter()
            .find(|entry| entry.func_id == func_id)
    }

    pub fn is_retired_prelude_id(func_id: FuncId) -> bool {
        retired_prelude_id(func_id).is_some()
    }
}

// ---------------------------------------------------------------------------
// LowerInput / Lowerer
// ---------------------------------------------------------------------------

/// Explicit inputs for multi-module lowering (replaces CompilationContext coupling).
pub struct LowerInput {
    pub type_env: TypeEnv,
    pub value_env: ValueEnv,
    pub func_table: HashMap<String, FuncId>,
    pub module_aliases: HashSet<String>,
    pub qualified_value_globals: HashMap<String, LocalId>,
    /// "alias.fn" → target module path + module-local FuncId.
    pub qualified_func_targets: HashMap<String, ExternalFuncRef>,
    /// Next FuncId to assign for hoisted lambdas / __init__
    pub next_func_id: u32,
    /// Starting GlobalLocal offset for this module's top-level lets
    pub next_global_local_id: u32,
}

pub struct Lowerer {
    type_map: TypeMap,
    type_env: TypeEnv,
    value_env: ValueEnv,
    /// Map from function name to its assigned FuncId
    func_table: HashMap<String, FuncId>,
    /// Set of module alias names (for cross-module call dispatch)
    module_aliases: HashSet<String>,
    errors: Vec<LowerError>,
    /// Per-function local variable allocator (reset for each function)
    local_allocator: LocalAllocator,
    /// Next FuncId for hoisted lambdas and __init__ (assigned after all user funcs)
    next_hoisted_id: u32,
    /// Hoisted lambda functions (and __init__) collected during lowering
    hoisted_functions: Vec<FunctionDef>,
    /// Module-level let bindings pre-allocated before any function lowering.
    /// Functions reference these via GlobalLocal(id); __init__ owns the actual Let nodes.
    module_globals: HashMap<String, LocalId>,
    /// Absolute next GlobalLocal id to allocate; starts at global_local_start and advances
    /// as module globals are collected.  Function allocators start here to avoid overlap.
    next_global_id: u32,
    /// The starting offset for this module's global LocalIds.
    /// For single-module compilation this is always 0.
    global_local_start: u32,
    /// "alias.name" → globally-unique LocalId for cross-module pub value references.
    qualified_value_globals: HashMap<String, LocalId>,
    /// "alias.fn" → target module path + module-local FuncId.
    qualified_func_targets: HashMap<String, ExternalFuncRef>,
    /// Placeholder FuncId → external target; linker resolves these.
    external_func_refs: HashMap<FuncId, ExternalFuncRef>,
    /// Reverse lookup to keep placeholder assignment stable per target.
    external_func_ref_ids: HashMap<ExternalFuncRef, FuncId>,
    /// Next placeholder FuncId for cross-module references.
    next_external_func_id: u32,
    /// True while lowering __init__ top-level statements; lets lowerer reuse pre-assigned
    /// module global LocalIds instead of allocating new ones.
    in_init_context: bool,
    /// Return type of the function currently being lowered (for `try Option` desugaring).
    current_fn_return_type: Option<MonoType>,
    current_type_param_bounds: HashMap<String, String>,
    /// Extern function import metadata keyed by FuncId.
    extern_imports: HashMap<FuncId, ExternImport>,
}

const EXTERNAL_FUNC_ID_START: u32 = 1_000_000_000;

impl Lowerer {
    pub fn new(type_map: TypeMap, type_env: TypeEnv) -> Self {
        let mut func_table = HashMap::new();
        registry::populate_func_table(&mut func_table, false);
        debug_assert!(
            !func_table
                .values()
                .any(|id| prelude::is_retired_prelude_id(*id)),
            "lowerer func_table must not contain retired prelude IDs"
        );

        // len is polymorphic and handled specially in lower_expr_call

        let module_aliases = registry::builtin_module_aliases()
            .iter()
            .map(|alias| (*alias).to_string())
            .collect();

        Self {
            type_map,
            type_env,
            value_env: ValueEnv::new(),
            func_table,
            module_aliases,
            errors: Vec::new(),
            local_allocator: LocalAllocator::new(),
            next_hoisted_id: prelude::USER_FUNC_START, // updated after user-func pass
            hoisted_functions: Vec::new(),
            module_globals: HashMap::new(),
            next_global_id: 0,
            global_local_start: 0,
            qualified_value_globals: HashMap::new(),
            qualified_func_targets: HashMap::new(),
            external_func_refs: HashMap::new(),
            external_func_ref_ids: HashMap::new(),
            next_external_func_id: EXTERNAL_FUNC_ID_START,
            in_init_context: false,
            current_fn_return_type: None,
            current_type_param_bounds: HashMap::new(),
            extern_imports: HashMap::new(),
        }
    }

    /// Construct a Lowerer from explicit inputs (multi-module mode).
    pub fn new_from_input(type_map: TypeMap, input: LowerInput) -> Self {
        Self {
            type_map,
            type_env: input.type_env,
            value_env: input.value_env,
            func_table: input.func_table,
            module_aliases: input.module_aliases,
            errors: Vec::new(),
            local_allocator: LocalAllocator::new(),
            next_hoisted_id: input.next_func_id,
            hoisted_functions: Vec::new(),
            module_globals: HashMap::new(),
            next_global_id: 0,
            global_local_start: input.next_global_local_id,
            qualified_value_globals: input.qualified_value_globals,
            qualified_func_targets: input.qualified_func_targets,
            external_func_refs: HashMap::new(),
            external_func_ref_ids: HashMap::new(),
            next_external_func_id: EXTERNAL_FUNC_ID_START,
            in_init_context: false,
            current_fn_return_type: None,
            current_type_param_bounds: HashMap::new(),
            extern_imports: HashMap::new(),
        }
    }

    /// Pre-scan top-level let bindings and assign stable LocalIds to them.
    /// Must be called before any function lowering so that functions can
    /// reference globals via GlobalLocal(id).
    fn collect_module_globals(&mut self, ast: &SourceFile) {
        let mut next_id = self.global_local_start;
        for item in &ast.items {
            if let Item::Stmt(Stmt::Let {
                pattern: Pattern::Ident(name, _),
                ..
            }) = item
            {
                self.module_globals.insert(name.clone(), LocalId(next_id));
                next_id += 1;
            }
        }
        self.next_global_id = next_id;
    }

    /// Collect extern function metadata for WASM import generation.
    fn collect_extern_import(&mut self, func_id: FuncId, decl: &ExternFunctionDecl) {
        let wasm_module = decl.module.clone();
        let qualified = format!("{}.{}", wasm_module, decl.name);

        // Resolve parameter types from the value_env signature (registered under qualified name)
        let sig = self.value_env.get_function(&qualified);
        let (param_tys, return_ty) = if let Some(sig) = sig {
            (sig.params.clone(), sig.ret.clone())
        } else {
            // Fallback — shouldn't happen if resolver ran correctly
            (vec![], None)
        };

        self.extern_imports.insert(
            func_id,
            ExternImport {
                wasm_module,
                wasm_name: decl.name.clone(),
                param_tys,
                return_ty,
            },
        );
    }

    /// Lower a complete source file to Core IR
    pub fn lower_module(mut self, ast: &SourceFile) -> Result<CoreModule, Vec<LowerError>> {
        // Pre-scan: assign stable LocalIds to module-level let bindings
        self.collect_module_globals(ast);

        // First pass: assign FuncIds to all user functions and extern fns (source order)
        // Skip any that are already in the func_table (pre-assigned by context)
        let mut next_func_id = prelude::USER_FUNC_START;
        for item in &ast.items {
            match item {
                Item::Function(decl) => {
                    if !self.func_table.contains_key(&decl.name) {
                        let func_id = FuncId(next_func_id);
                        self.func_table.insert(decl.name.clone(), func_id);
                    }
                    // Advance counter past any pre-assigned IDs
                    if let Some(existing) = self.func_table.get(&decl.name) {
                        if existing.0 >= next_func_id {
                            next_func_id = existing.0 + 1;
                        }
                    }
                }
                Item::ExternFunction(decl) => {
                    let qualified = format!("{}.{}", decl.module, decl.name);
                    if !self.func_table.contains_key(&qualified) {
                        let func_id = FuncId(next_func_id);
                        self.func_table.insert(qualified, func_id);
                        next_func_id += 1;
                        self.collect_extern_import(func_id, decl);
                    }
                }
                _ => {}
            }
        }
        self.next_hoisted_id = next_func_id;

        // Second pass: lower each function
        let mut functions = Vec::new();
        for item in &ast.items {
            if let Item::Function(decl) = item {
                if let Some(func_def) = self.lower_function(decl) {
                    functions.push(func_def);
                }
            }
        }

        // Lower top-level statements into __init__
        let init_func_id = self.lower_init_stmts(ast).map(|init_def| {
            let id = init_def.func_id;
            self.hoisted_functions.push(init_def);
            id
        });

        // Include hoisted lambdas and __init__ in the function list
        functions.extend(self.hoisted_functions.drain(..));

        if self.errors.is_empty() {
            let all_init_func_ids = init_func_id.into_iter().collect();
            Ok(CoreModule {
                functions,
                type_env: self.type_env,
                init_func_id,
                all_init_func_ids,
                extern_imports: self.extern_imports,
            })
        } else {
            Err(self.errors)
        }
    }

    /// Lower only the functions of a source file, returning them as a Vec along
    /// with the Optional FuncId of the __init__ function (top-level stmts).
    /// FuncIds for user functions must already be pre-assigned in `self.func_table`.
    /// Used by the multi-module pipeline.
    /// Returns (functions, init_func_id, next_global_id, next_hoisted_id).
    /// - `next_global_id`: write back to the accumulator's `next_global_local_id`
    /// - `next_hoisted_id`: write back to the accumulator's `next_func_id` so the next
    ///   module's hoisted functions (lambdas, __init__) don't reuse the same FuncIds.
    pub fn lower_module_funcs(
        mut self,
        ast: &SourceFile,
    ) -> Result<LoweredModule, Vec<LowerError>> {
        // Pre-scan: assign stable LocalIds to module-level let bindings
        self.collect_module_globals(ast);

        // Collect extern import metadata for pre-assigned extern function FuncIds
        for item in &ast.items {
            if let Item::ExternFunction(decl) = item {
                let qualified = format!("{}.{}", decl.module, decl.name);
                if let Some(&func_id) = self.func_table.get(&qualified) {
                    self.collect_extern_import(func_id, decl);
                }
            }
        }

        let mut functions = Vec::new();
        for item in &ast.items {
            if let Item::Function(decl) = item {
                if let Some(func_def) = self.lower_function(decl) {
                    functions.push(func_def);
                }
            }
        }

        // Lower top-level statements into __init__
        let init_func_id = self.lower_init_stmts(ast).map(|init_def| {
            let id = init_def.func_id;
            functions.push(init_def);
            id
        });

        // Include any hoisted lambdas
        functions.extend(self.hoisted_functions.drain(..));

        if self.errors.is_empty() {
            Ok(LoweredModule {
                module_path: std::path::PathBuf::new(),
                dependencies: Vec::new(),
                functions,
                init_func_id,
                external_func_refs: self.external_func_refs,
                extern_imports: self.extern_imports,
                next_func_id_after: self.next_hoisted_id,
                next_global_local_id_after: self.next_global_id,
            })
        } else {
            Err(self.errors)
        }
    }

    /// Allocate the next FuncId for a hoisted function (lambda or __init__).
    fn alloc_hoisted_id(&mut self) -> FuncId {
        let id = FuncId(self.next_hoisted_id);
        self.next_hoisted_id += 1;
        id
    }

    fn resolves_as_value_binding(&self, name: &str) -> bool {
        self.local_allocator.lookup(name).is_some() || self.module_globals.contains_key(name)
    }

    fn can_use_module_alias(&self, name: &str) -> bool {
        (self.module_aliases.contains(name) || self.value_env.is_extern_namespace(name))
            && !self.resolves_as_value_binding(name)
    }

    /// Extract a dotted name from an Ident/FieldAccess chain and look it up as a type.
    /// Handles `TypeName` (bare Ident, if not a local/func) and `module.TypeName` (dotted path).
    fn try_resolve_type_from_expr(&self, expr: &Expr) -> Option<TypeId> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                // Only treat as type if not shadowed by a local or function
                if self.local_allocator.lookup(name).is_none()
                    && !self.func_table.contains_key(name.as_str())
                {
                    self.type_env.lookup_type(name)
                } else {
                    None
                }
            }
            ExprKind::FieldAccess { .. } => {
                let dotted = expr_as_dotted_name(expr)?;
                self.type_env.lookup_type(&dotted)
            }
            _ => None,
        }
    }

    fn resolve_named_func_id(&mut self, name: &str, span: Span) -> Option<FuncId> {
        if let Some(target) = self.qualified_func_targets.get(name).cloned() {
            return Some(self.alloc_external_func_id(target));
        }
        if let Some(&func_id) = self.func_table.get(name) {
            return Some(func_id);
        }
        self.errors.push(LowerError::InternalError {
            message: format!("no FuncId for '{}'", name),
            span,
        });
        None
    }

    fn resolve_registered_method_func_id(
        &mut self,
        receiver_ty: &MonoType,
        method: &str,
        span: Span,
    ) -> Option<FuncId> {
        let receiver_type_id = method_receiver_type_id(receiver_ty)?;
        let func_name = self
            .type_env
            .get_method_function(receiver_type_id, method)
            .cloned()?;
        self.resolve_named_func_id(&func_name, span)
    }

    fn alloc_external_func_id(&mut self, target: ExternalFuncRef) -> FuncId {
        if let Some(&existing) = self.external_func_ref_ids.get(&target) {
            return existing;
        }
        let id = FuncId(self.next_external_func_id);
        self.next_external_func_id += 1;
        self.external_func_ref_ids.insert(target.clone(), id);
        self.external_func_refs.insert(id, target);
        id
    }

    /// Lower all top-level expression statements into a synthetic `__init__` function.
    /// Returns `None` if there are no top-level statements.
    fn lower_init_stmts(&mut self, ast: &SourceFile) -> Option<FunctionDef> {
        // Collect all top-level statements in source order
        let stmts: Vec<&Stmt> = ast
            .items
            .iter()
            .filter_map(|item| {
                if let Item::Stmt(s) = item {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();

        if stmts.is_empty() {
            return None;
        }

        // Allocator for __init__: new locals start after the globals range,
        // but all module globals are pre-bound to their stable LocalIds.
        self.local_allocator = LocalAllocator::new_at(self.next_global_id);
        for (name, &local_id) in &self.module_globals {
            self.local_allocator.bind(name.clone(), local_id);
        }

        // We need an owned Vec<Stmt> to call lower_stmts — clone for now
        let stmts_owned: Vec<Stmt> = stmts.iter().map(|s| (*s).clone()).collect();

        let span = match stmts_owned.first() {
            Some(s) => stmt_span(s),
            None => return None,
        };

        self.in_init_context = true;
        let body = self.lower_stmts(&stmts_owned, span)?;
        self.in_init_context = false;
        let func_id = self.alloc_hoisted_id();

        Some(FunctionDef {
            func_id,
            name: "__init__".to_string(),
            params: vec![],
            param_tys: vec![],
            return_ty: MonoType::Void,
            body,
        })
    }

    // -----------------------------------------------------------------------
    // Function lowering
    // -----------------------------------------------------------------------

    fn lower_function(&mut self, decl: &FunctionDecl) -> Option<FunctionDef> {
        // Fresh LocalAllocator starting after the module globals range
        self.local_allocator = LocalAllocator::new_at(self.next_global_id);

        // Bind parameters
        let mut params = Vec::new();
        for param in &decl.params {
            let local_id = self.local_allocator.alloc_and_bind(param.name.clone());
            params.push(local_id);
        }

        // Extract param types from the ValueEnv function signature
        let param_tys = self
            .value_env
            .get_function(&decl.name)
            .map(|sig| sig.params.clone())
            .unwrap_or_default();

        // Track the return type for `try Option` desugaring
        self.current_fn_return_type = self
            .value_env
            .get_function(&decl.name)
            .and_then(|sig| sig.ret.clone());
        let saved_type_param_bounds = std::mem::replace(
            &mut self.current_type_param_bounds,
            decl.type_params
                .iter()
                .filter_map(|p| {
                    p.bound
                        .as_ref()
                        .map(|bound| (p.name.clone(), bound.clone()))
                })
                .collect(),
        );

        let body = self.lower_block(&decl.body)?;
        // When the body ends with a `return` (type Never) or is Void but the
        // function has a declared return type, use the signature's return type
        // so the Wasm codegen emits the correct result type.
        let return_ty = match &body.ty {
            MonoType::Never | MonoType::Void => self
                .value_env
                .get_function(&decl.name)
                .and_then(|sig| sig.ret.clone())
                .unwrap_or_else(|| body.ty.clone()),
            _ => body.ty.clone(),
        };
        let func_id = *self.func_table.get(&decl.name)?;
        let qualified_name = self
            .func_table
            .iter()
            .filter_map(|(name, id)| {
                if *id == func_id
                    && name != &decl.name
                    && name
                        .strip_suffix(&decl.name)
                        .is_some_and(|prefix| prefix.ends_with('.'))
                {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .min_by_key(|name| name.len())
            .unwrap_or_else(|| decl.name.clone());

        self.current_type_param_bounds = saved_type_param_bounds;

        Some(FunctionDef {
            func_id,
            name: qualified_name,
            params,
            param_tys,
            body,
            return_ty,
        })
    }

    // -----------------------------------------------------------------------
    // Block / statement lowering
    // -----------------------------------------------------------------------

    fn lower_block(&mut self, block: &Block) -> Option<CoreExpr> {
        self.lower_stmts(&block.stmts, block.span)
    }

    fn lower_stmts(&mut self, stmts: &[Stmt], span: Span) -> Option<CoreExpr> {
        match stmts {
            [] => Some(CoreExpr {
                kind: CoreExprKind::LitVoid,
                ty: MonoType::Void,
                span,
            }),

            [stmt, rest @ ..] => self.lower_stmt_head(stmt, rest, span),
        }
    }

    fn lower_stmt_head(&mut self, stmt: &Stmt, rest: &[Stmt], span: Span) -> Option<CoreExpr> {
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                // Check for simple rebinding: `x = expr` is a BinOp::Assign but
                // Stmt::Let without ColonEq token is treated as a new binding here.
                // All Stmt::Let create new bindings.
                match pattern {
                    Pattern::Ident(name, _) => {
                        let value_expr = self.lower_expr(value)?;
                        // In __init__ context, module globals already have stable LocalIds.
                        // Reuse the pre-assigned id so functions' GlobalLocal(id) references match.
                        let local = if self.in_init_context {
                            if let Some(&pre_id) = self.module_globals.get(name.as_str()) {
                                pre_id
                            } else {
                                self.local_allocator.alloc_and_bind(name.clone())
                            }
                        } else {
                            self.local_allocator.alloc_and_bind(name.clone())
                        };
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local,
                                value: Box::new(value_expr),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        })
                    }
                    Pattern::Wildcard(_) => {
                        let value_expr = self.lower_expr(value)?;
                        let local = self.local_allocator.alloc();
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local,
                                value: Box::new(value_expr),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        })
                    }
                    _ => {
                        self.errors.push(LowerError::UnsupportedFeature {
                            feature: "complex pattern in let binding",
                            span: stmt_span(stmt),
                        });
                        None
                    }
                }
            }

            Stmt::Expr(e) => {
                // Check if this is a rebinding assignment: `x = expr`
                if let Some((name, value)) = extract_simple_assign(e) {
                    // Look up the existing LocalId for this name
                    if let Some(existing_local) = self.local_allocator.lookup(name) {
                        let value_expr = self.lower_expr(value)?;
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        // Assign mutates the existing slot; no new LocalId allocated
                        return Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: self.local_allocator.alloc(),
                                value: Box::new(CoreExpr {
                                    kind: CoreExprKind::Assign {
                                        local: existing_local,
                                        value: Box::new(value_expr),
                                    },
                                    ty: MonoType::Void,
                                    span,
                                }),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        });
                    }
                }

                // Non-ident lvalue assignment: r.field = val, arr[i] = val, m[k] = val
                if let ExprKind::Binary {
                    op: BinOp::Assign,
                    left,
                    right,
                } = &e.kind
                {
                    let rhs_core = self.lower_expr(right)?;
                    if let Some((root_local, update_expr)) = self.lower_lvalue_chain(left, rhs_core)
                    {
                        let body = self.lower_stmts(rest, span)?;
                        let ty = body.ty.clone();
                        return Some(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: self.local_allocator.alloc(),
                                value: Box::new(CoreExpr {
                                    kind: CoreExprKind::Assign {
                                        local: root_local,
                                        value: Box::new(update_expr),
                                    },
                                    ty: MonoType::Void,
                                    span,
                                }),
                                body: Box::new(body),
                            },
                            ty,
                            span,
                        });
                    }
                }

                if rest.is_empty() {
                    // Last statement: block value
                    self.lower_expr(e)
                } else {
                    // Side-effect expression, then continue
                    let e_expr = self.lower_expr(e)?;
                    let body = self.lower_stmts(rest, span)?;
                    let ty = body.ty.clone();
                    let temp = self.local_allocator.alloc();
                    Some(CoreExpr {
                        kind: CoreExprKind::Let {
                            local: temp,
                            value: Box::new(e_expr),
                            body: Box::new(body),
                        },
                        ty,
                        span,
                    })
                }
            }

            Stmt::Return {
                value,
                span: ret_span,
            } => {
                let val = match value {
                    Some(v) => Some(Box::new(self.lower_expr(v)?)),
                    None => None,
                };
                Some(CoreExpr {
                    kind: CoreExprKind::Return { value: val },
                    ty: MonoType::Never,
                    span: *ret_span,
                })
            }

            Stmt::Break {
                value,
                span: brk_span,
            } => {
                let val = match value {
                    Some(v) => Some(Box::new(self.lower_expr(v)?)),
                    None => None,
                };
                Some(CoreExpr {
                    kind: CoreExprKind::Break { value: val },
                    ty: MonoType::Void,
                    span: *brk_span,
                })
            }

            Stmt::Continue { span: cont_span } => Some(CoreExpr {
                kind: CoreExprKind::Continue,
                ty: MonoType::Void,
                span: *cont_span,
            }),

            Stmt::Defer {
                expr,
                span: defer_span,
            } => {
                let deferred_core = self.lower_expr(expr)?;
                let body = self.lower_stmts(rest, span)?;
                let ty = body.ty.clone();
                let tmp = self.local_allocator.alloc();
                Some(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: tmp,
                        value: Box::new(CoreExpr {
                            kind: CoreExprKind::Defer(Box::new(deferred_core)),
                            ty: MonoType::Void,
                            span: *defer_span,
                        }),
                        body: Box::new(body),
                    },
                    ty,
                    span,
                })
            }

            Stmt::For { span: for_span, .. } | Stmt::ForCond { span: for_span, .. } => {
                self.lower_for_stmt(stmt, rest, span, *for_span)
            }
        }
    }

    fn lower_for_stmt(
        &mut self,
        stmt: &Stmt,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        match stmt {
            Stmt::ForCond { cond, body, .. } => {
                // for cond { body }
                // → Loop { If { cond: !cond, then: Break(None), else: body } }
                let cond_expr = self.lower_expr(cond)?;
                let not_cond = CoreExpr {
                    ty: MonoType::Bool,
                    span: cond_expr.span,
                    kind: CoreExprKind::UnOp {
                        op: crate::syntax::ast::UnOp::Not,
                        expr: Box::new(cond_expr),
                    },
                };
                let break_expr = CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: for_span,
                };

                self.local_allocator.push_scope();
                let body_expr = self.lower_block(body)?;
                self.local_allocator.pop_scope();

                let if_expr = CoreExpr {
                    ty: MonoType::Void,
                    span: for_span,
                    kind: CoreExprKind::If {
                        cond: Box::new(not_cond),
                        then_branch: Box::new(break_expr),
                        else_branch: Box::new(body_expr),
                    },
                };

                let loop_expr = CoreExpr {
                    kind: CoreExprKind::Loop {
                        body: Box::new(if_expr),
                    },
                    ty: MonoType::Void,
                    span: for_span,
                };

                // The loop is a statement; continue with rest
                let continuation = self.lower_stmts(rest, cont_span)?;
                let ty = continuation.ty.clone();
                let temp = self.local_allocator.alloc();
                Some(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: temp,
                        value: Box::new(loop_expr),
                        body: Box::new(continuation),
                    },
                    ty,
                    span: for_span,
                })
            }

            Stmt::For {
                pattern,
                index_pattern,
                iter,
                body,
                ..
            } => {
                let iter_ty = self.type_map.get_expr_type(iter.id).cloned();
                if matches!(iter_ty, Some(MonoType::Dict(_, _))) {
                    return self.lower_dict_for_stmt(
                        pattern,
                        index_pattern.as_ref(),
                        iter,
                        body,
                        rest,
                        cont_span,
                        for_span,
                    );
                }
                if matches!(iter_ty, Some(MonoType::Named { type_id, .. }) if type_id == ITERATOR_TYPE_ID)
                {
                    return self.lower_iterator_for_stmt(
                        pattern,
                        index_pattern.as_ref(),
                        iter,
                        body,
                        rest,
                        cont_span,
                        for_span,
                    );
                }
                if matches!(iter_ty, Some(MonoType::Named { type_id, .. }) if type_id == RANGE_TYPE_ID)
                {
                    return self.lower_range_for_stmt(
                        pattern,
                        index_pattern.as_ref(),
                        iter,
                        body,
                        rest,
                        cont_span,
                        for_span,
                    );
                }

                // for x in arr { body }  (and  for x, i in arr { body })
                // → Let(arr_tmp, iter,
                //     Let(len_tmp, array_len(arr_tmp),
                //       Let(idx_tmp, 0,
                //         Loop {
                //           If(idx_tmp >= len_tmp,
                //             Break(None),
                //             Let(elem, arr_tmp[idx_tmp],
                //               <body>_then_Let(idx_tmp2, idx_tmp+1, <rebind idx_tmp, Continue>)))
                //         })))
                let iter_expr = self.lower_expr(iter)?;
                let iter_span = iter.span;

                // arr_tmp = iter
                let arr_tmp = self.local_allocator.alloc_and_bind("__arr".to_string());
                let len_tmp = self.local_allocator.alloc_and_bind("__len".to_string());
                let idx_tmp = self.local_allocator.alloc_and_bind("__idx".to_string());

                let (len_func_id, elem_ty) = match &iter_expr.ty {
                    MonoType::Vector(inner) => (prelude::VECTOR_LEN, *inner.clone()),
                    MonoType::String => (prelude::STRING_LEN, MonoType::Byte),
                    _ => (prelude::VECTOR_LEN, MonoType::Void),
                };

                // length call
                let arr_len_func = CoreExpr {
                    kind: CoreExprKind::GlobalFunc(len_func_id),
                    ty: MonoType::Function {
                        params: vec![iter_expr.ty.clone()],
                        ret: Box::new(MonoType::Int),
                    },
                    span: iter_span,
                };
                let arr_local_expr = CoreExpr {
                    kind: CoreExprKind::Local(arr_tmp),
                    ty: iter_expr.ty.clone(),
                    span: iter_span,
                };
                let len_call = CoreExpr {
                    kind: CoreExprKind::Call {
                        callee: Box::new(arr_len_func),
                        args: vec![arr_local_expr.clone()],
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                };

                // Loop body
                self.local_allocator.push_scope();

                // Bind element variable
                let elem_local = match pattern {
                    Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
                    _ => self.local_allocator.alloc(),
                };

                // Optionally bind index variable
                let idx_user = index_pattern.as_ref().and_then(|ip| {
                    if let Pattern::Ident(name, _) = ip {
                        Some(self.local_allocator.alloc_and_bind(name.clone()))
                    } else {
                        None
                    }
                });

                // elem = arr_tmp[idx_tmp]
                let idx_local_expr = CoreExpr {
                    kind: CoreExprKind::Local(idx_tmp),
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let elem_value = self.lower_index_core_expr(
                    arr_local_expr,
                    idx_local_expr.clone(),
                    elem_ty.clone(),
                    iter_span,
                );

                let body_expr = self.lower_block(body)?;
                self.local_allocator.pop_scope();

                // Increment idx using Assign (mutation) then Continue
                let one = CoreExpr {
                    kind: CoreExprKind::LitInt(1),
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let idx_plus_one = CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Add,
                        left: Box::new(idx_local_expr.clone()),
                        right: Box::new(one),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let continue_expr = CoreExpr {
                    kind: CoreExprKind::Continue,
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Assign(idx_tmp, idx+1) then Continue
                let idx_inc = CoreExpr {
                    kind: CoreExprKind::Assign {
                        local: idx_tmp,
                        value: Box::new(idx_plus_one),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };
                let tmp_after_inc = self.local_allocator.alloc();

                // body then continue
                let body_then_cont = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(body_expr),
                        body: Box::new(continue_expr),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Assign(idx, idx+1) BEFORE body so that if body hits `continue`
                // the index is already advanced to the next element.
                let inc_then_body = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: tmp_after_inc,
                        value: Box::new(idx_inc),
                        body: Box::new(body_then_cont),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Optionally bind user-visible index BEFORE the increment
                let loop_body_inner = if let Some(user_idx) = idx_user {
                    CoreExpr {
                        kind: CoreExprKind::Let {
                            local: user_idx,
                            value: Box::new(idx_local_expr.clone()),
                            body: Box::new(inc_then_body),
                        },
                        ty: MonoType::Void,
                        span: iter_span,
                    }
                } else {
                    inc_then_body
                };

                // Let(elem, elem_value, loop_body_inner)
                let elem_let = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: elem_local,
                        value: Box::new(elem_value),
                        body: Box::new(loop_body_inner),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                // Break when idx >= len
                let len_local_expr = CoreExpr {
                    kind: CoreExprKind::Local(len_tmp),
                    ty: MonoType::Int,
                    span: iter_span,
                };
                let cond_expr = CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Ge,
                        left: Box::new(idx_local_expr),
                        right: Box::new(len_local_expr),
                    },
                    ty: MonoType::Bool,
                    span: iter_span,
                };
                let break_expr = CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: iter_span,
                };
                let if_expr = CoreExpr {
                    kind: CoreExprKind::If {
                        cond: Box::new(cond_expr),
                        then_branch: Box::new(break_expr),
                        else_branch: Box::new(elem_let),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                };

                let loop_expr = CoreExpr {
                    kind: CoreExprKind::Loop {
                        body: Box::new(if_expr),
                    },
                    ty: MonoType::Void,
                    span: for_span,
                };

                // Wrap in Let(arr_tmp, iter, Let(len_tmp, len, Let(idx_tmp, 0, loop)))
                let zero = CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                };

                // Add continuation after the loop
                let continuation = self.lower_stmts(rest, cont_span)?;
                let cont_ty = continuation.ty.clone();
                let loop_then_cont = CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(loop_expr),
                        body: Box::new(continuation),
                    },
                    ty: cont_ty.clone(),
                    span: for_span,
                };

                Some(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: arr_tmp,
                        value: Box::new(iter_expr),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: len_tmp,
                                value: Box::new(len_call),
                                body: Box::new(CoreExpr {
                                    kind: CoreExprKind::Let {
                                        local: idx_tmp,
                                        value: Box::new(zero),
                                        body: Box::new(loop_then_cont),
                                    },
                                    ty: cont_ty.clone(),
                                    span: for_span,
                                }),
                            },
                            ty: cont_ty.clone(),
                            span: for_span,
                        }),
                    },
                    ty: cont_ty,
                    span: for_span,
                })
            }

            _ => unreachable!(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_dict_for_stmt(
        &mut self,
        pattern: &Pattern,
        val_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        // for k, v in dict { body }
        // → Let(dict_tmp, dict,
        //     Let(keys_tmp, Dict.keys(dict_tmp),
        //       Let(len_tmp, array_len(keys_tmp),
        //         Let(idx_tmp, 0,
        //           Loop {
        //             If(idx_tmp >= len_tmp, Break,
        //               Let(k, keys_tmp[idx_tmp],
        //                 [Let(v, dict_tmp[k],]
        //                   body_then_Assign(idx_tmp, idx_tmp+1)_Continue))
        //           }))))
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let (key_ty, val_ty) = match &iter_expr.ty {
            MonoType::Dict(k, v) => (*k.clone(), *v.clone()),
            _ => unreachable!(),
        };

        let dict_tmp = self.local_allocator.alloc_and_bind("__dict".to_string());
        let keys_tmp = self.local_allocator.alloc_and_bind("__keys".to_string());
        let len_tmp = self.local_allocator.alloc_and_bind("__len".to_string());
        let idx_tmp = self.local_allocator.alloc_and_bind("__idx".to_string());

        let dict_local_expr = CoreExpr {
            kind: CoreExprKind::Local(dict_tmp),
            ty: iter_expr.ty.clone(),
            span: iter_span,
        };
        let keys_ty = MonoType::Vector(Box::new(key_ty.clone()));

        // keys = Dict.keys(dict_tmp)
        let dict_keys_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::DICT_KEYS),
            ty: MonoType::Function {
                params: vec![iter_expr.ty.clone()],
                ret: Box::new(keys_ty.clone()),
            },
            span: iter_span,
        };
        let keys_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(dict_keys_func),
                args: vec![dict_local_expr.clone()],
            },
            ty: keys_ty.clone(),
            span: iter_span,
        };

        let keys_local_expr = CoreExpr {
            kind: CoreExprKind::Local(keys_tmp),
            ty: keys_ty.clone(),
            span: iter_span,
        };

        // len = array_len(keys_tmp)
        let arr_len_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::VECTOR_LEN),
            ty: MonoType::Function {
                params: vec![keys_ty.clone()],
                ret: Box::new(MonoType::Int),
            },
            span: iter_span,
        };
        let len_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(arr_len_func),
                args: vec![keys_local_expr.clone()],
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        let idx_expr = CoreExpr {
            kind: CoreExprKind::Local(idx_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let len_expr = CoreExpr {
            kind: CoreExprKind::Local(len_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };

        // Loop body: allocate k and v inside scope so body can reference them
        self.local_allocator.push_scope();

        let key_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };
        let val_local = val_pattern.map(|vp| match vp {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        });

        let body_expr = self.lower_block(body)?;
        self.local_allocator.pop_scope();

        // key_value = keys_tmp[idx_tmp]
        let key_value = CoreExpr {
            kind: CoreExprKind::Index {
                base: Box::new(keys_local_expr),
                index: Box::new(idx_expr.clone()),
            },
            ty: key_ty.clone(),
            span: iter_span,
        };

        // Increment: Assign(idx_tmp, idx_tmp + 1) then Continue
        let idx_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: idx_tmp,
                value: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Add,
                        left: Box::new(idx_expr),
                        right: Box::new(CoreExpr {
                            kind: CoreExprKind::LitInt(1),
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        // Increment idx BEFORE body so that if body hits `continue`
        // the index is already advanced to the next element.
        let body_then_tail = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(idx_inc),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(body_expr),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Continue,
                            ty: MonoType::Void,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Optionally wrap with: Let(v, dict_get_unsafe(dict_tmp, k), body_then_tail)
        let loop_body_inner = if let Some(vl) = val_local {
            let dict_get_unsafe_func = CoreExpr {
                kind: CoreExprKind::GlobalFunc(prelude::DICT_GET_UNSAFE),
                ty: MonoType::Function {
                    params: vec![iter_expr.ty.clone(), key_ty.clone()],
                    ret: Box::new(val_ty.clone()),
                },
                span: iter_span,
            };
            let val_value = CoreExpr {
                kind: CoreExprKind::Call {
                    callee: Box::new(dict_get_unsafe_func),
                    args: vec![
                        dict_local_expr,
                        CoreExpr {
                            kind: CoreExprKind::Local(key_local),
                            ty: key_ty,
                            span: iter_span,
                        },
                    ],
                },
                ty: val_ty,
                span: iter_span,
            };
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: vl,
                    value: Box::new(val_value),
                    body: Box::new(body_then_tail),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            body_then_tail
        };

        // Let(k, keys[idx], loop_body_inner)
        let key_let = CoreExpr {
            kind: CoreExprKind::Let {
                local: key_local,
                value: Box::new(key_value),
                body: Box::new(loop_body_inner),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // If(idx >= len, Break, key_let)
        let cond_expr = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Ge,
                left: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(idx_tmp),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
                right: Box::new(len_expr),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let if_expr = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(cond_expr),
                then_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: for_span,
                }),
                else_branch: Box::new(key_let),
            },
            ty: MonoType::Void,
            span: for_span,
        };

        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(if_expr),
            },
            ty: MonoType::Void,
            span: for_span,
        };

        // Continuation after the loop
        let continuation = self.lower_stmts(rest, cont_span)?;
        let cont_ty = continuation.ty.clone();
        let loop_then_cont = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(loop_expr),
                body: Box::new(continuation),
            },
            ty: cont_ty.clone(),
            span: for_span,
        };

        // Wrap: Let(dict, iter, Let(keys, Dict.keys(dict), Let(len, ..., Let(idx, 0, ...))))
        let zero = CoreExpr {
            kind: CoreExprKind::LitInt(0),
            ty: MonoType::Int,
            span: iter_span,
        };
        Some(CoreExpr {
            kind: CoreExprKind::Let {
                local: dict_tmp,
                value: Box::new(iter_expr),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: keys_tmp,
                        value: Box::new(keys_call),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: len_tmp,
                                value: Box::new(len_call),
                                body: Box::new(CoreExpr {
                                    kind: CoreExprKind::Let {
                                        local: idx_tmp,
                                        value: Box::new(zero),
                                        body: Box::new(loop_then_cont),
                                    },
                                    ty: cont_ty.clone(),
                                    span: for_span,
                                }),
                            },
                            ty: cont_ty.clone(),
                            span: for_span,
                        }),
                    },
                    ty: cont_ty.clone(),
                    span: for_span,
                }),
            },
            ty: cont_ty,
            span: for_span,
        })
    }

    // -----------------------------------------------------------------------
    // Iterator for-loop lowering
    // -----------------------------------------------------------------------

    /// Lower `for x in iter { body }` where iter: Iterator<T>.
    ///
    /// Emits:
    ///   Let(loop_it, iter_expr,
    ///     Let(_, rest_stmts,
    ///       Loop {
    ///         Let(opt, Call(ITERATOR_NEXT, [Local(loop_it)]),
    ///           Match(opt) {
    ///             None   → Break(None)
    ///             Some(item) →
    ///               Let(x, RecordGet(item, 0),
    ///                 Let(_, Assign(loop_it, RecordGet(item, 1)),
    ///                   Let(_, body, Continue)))
    ///           })
    ///       }))
    #[allow(clippy::too_many_arguments)]
    fn lower_iterator_for_stmt(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let elem_ty = match &iter_expr.ty {
            MonoType::Named { type_id, args } if *type_id == ITERATOR_TYPE_ID => {
                args.first().cloned().unwrap_or(MonoType::Void)
            }
            _ => MonoType::Void,
        };
        let iter_ty = iter_expr.ty.clone();
        let option_item_ty = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![MonoType::Named {
                type_id: ITER_ITEM_TYPE_ID,
                args: vec![elem_ty.clone()],
            }],
        };
        let item_ty = MonoType::Named {
            type_id: ITER_ITEM_TYPE_ID,
            args: vec![elem_ty.clone()],
        };

        // loop_it: mutable local holding the current iterator state
        let loop_it = self.local_allocator.alloc_and_bind("__it".to_string());

        // idx_counter: mutable local for the iteration index (allocated if index_pattern present)
        let idx_counter =
            index_pattern.map(|_| self.local_allocator.alloc_and_bind("__idx".to_string()));

        // Loop scope
        self.local_allocator.push_scope();

        let opt_local = self.local_allocator.alloc_and_bind("__opt".to_string());
        let item_local = self.local_allocator.alloc_and_bind("__item".to_string());

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };

        let idx_user = index_pattern.and_then(|ip| {
            if let Pattern::Ident(name, _) = ip {
                Some(self.local_allocator.alloc_and_bind(name.clone()))
            } else {
                None
            }
        });

        let body_expr = self.lower_block(body)?;
        self.local_allocator.pop_scope();

        // Call(ITERATOR_NEXT, [Local(loop_it)])
        let next_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::ITERATOR_NEXT),
                    ty: MonoType::Function {
                        params: vec![iter_ty.clone()],
                        ret: Box::new(option_item_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![CoreExpr {
                    kind: CoreExprKind::Local(loop_it),
                    ty: iter_ty.clone(),
                    span: iter_span,
                }],
            },
            ty: option_item_ty,
            span: iter_span,
        };

        // RecordGet(Local(item_local), 0) → value: T
        let value_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(item_local),
                    ty: item_ty.clone(),
                    span: iter_span,
                }),
                field: FieldId(0),
            },
            ty: elem_ty,
            span: iter_span,
        };
        // RecordGet(Local(item_local), 1) → rest: Iterator<T>
        let rest_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(item_local),
                    ty: item_ty,
                    span: iter_span,
                }),
                field: FieldId(1),
            },
            ty: iter_ty.clone(),
            span: iter_span,
        };
        // Assign(loop_it, rest_get)
        let advance_it = CoreExpr {
            kind: CoreExprKind::Assign {
                local: loop_it,
                value: Box::new(rest_get),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let continue_expr = CoreExpr {
            kind: CoreExprKind::Continue,
            ty: MonoType::Void,
            span: iter_span,
        };

        // Build the tail: idx_increment → body → continue
        let tail = if let Some(idx_ctr) = idx_counter {
            // Let(_, Assign(idx_counter, idx_counter + 1), Continue)
            let idx_inc = CoreExpr {
                kind: CoreExprKind::Assign {
                    local: idx_ctr,
                    value: Box::new(CoreExpr {
                        kind: CoreExprKind::BinOp {
                            op: BinOp::Add,
                            left: Box::new(CoreExpr {
                                kind: CoreExprKind::Local(idx_ctr),
                                ty: MonoType::Int,
                                span: iter_span,
                            }),
                            right: Box::new(CoreExpr {
                                kind: CoreExprKind::LitInt(1),
                                ty: MonoType::Int,
                                span: iter_span,
                            }),
                        },
                        ty: MonoType::Int,
                        span: iter_span,
                    }),
                },
                ty: MonoType::Void,
                span: iter_span,
            };
            let inc_then_cont = CoreExpr {
                kind: CoreExprKind::Let {
                    local: self.local_allocator.alloc(),
                    value: Box::new(idx_inc),
                    body: Box::new(continue_expr),
                },
                ty: MonoType::Void,
                span: iter_span,
            };
            let body_then_inc = CoreExpr {
                kind: CoreExprKind::Let {
                    local: self.local_allocator.alloc(),
                    value: Box::new(body_expr),
                    body: Box::new(inc_then_cont),
                },
                ty: MonoType::Void,
                span: iter_span,
            };
            body_then_inc
        } else {
            // No index: body → continue
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: self.local_allocator.alloc(),
                    value: Box::new(body_expr),
                    body: Box::new(continue_expr),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        };

        // Wrap with advance_it before the tail
        let with_advance = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(advance_it),
                body: Box::new(tail),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Optionally bind user index local
        let with_idx = if let Some(idx_u) = idx_user {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: idx_u,
                    value: Box::new(CoreExpr {
                        kind: CoreExprKind::Local(idx_counter.unwrap()),
                        ty: MonoType::Int,
                        span: iter_span,
                    }),
                    body: Box::new(with_advance),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            with_advance
        };

        // Let(x, value_get, with_idx)
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(value_get),
                body: Box::new(with_idx),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Match arms:
        //   VariantId(0) = None  → Break(None)
        //   VariantId(1) = Some  → Let(item, payload, with_elem)
        let break_expr = CoreExpr {
            kind: CoreExprKind::Break { value: None },
            ty: MonoType::Void,
            span: for_span,
        };
        let match_expr = CoreExpr {
            kind: CoreExprKind::Match {
                scrutinee: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(opt_local),
                    ty: MonoType::Named {
                        type_id: OPTION_TYPE_ID,
                        args: vec![MonoType::Void],
                    },
                    span: iter_span,
                }),
                arms: vec![
                    MatchArm {
                        pattern: CorePattern::Variant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(0),
                            fields: vec![],
                        },
                        body: break_expr,
                    },
                    MatchArm {
                        pattern: CorePattern::Variant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(1),
                            fields: vec![CorePattern::Var(item_local)],
                        },
                        body: with_elem,
                    },
                ],
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(opt, next_call, match_expr)
        let loop_body = CoreExpr {
            kind: CoreExprKind::Let {
                local: opt_local,
                value: Box::new(next_call),
                body: Box::new(match_expr),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_body),
            },
            ty: MonoType::Void,
            span: for_span,
        };

        // Continuation after the loop
        let continuation = self.lower_stmts(rest, cont_span)?;
        let cont_ty = continuation.ty.clone();
        let loop_then_cont = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(loop_expr),
                body: Box::new(continuation),
            },
            ty: cont_ty.clone(),
            span: for_span,
        };

        // Let(loop_it, iter_expr, ...)
        let with_iter = CoreExpr {
            kind: CoreExprKind::Let {
                local: loop_it,
                value: Box::new(iter_expr),
                body: Box::new(loop_then_cont),
            },
            ty: cont_ty.clone(),
            span: for_span,
        };

        // Optionally wrap with Let(idx_counter, 0, ...)
        if let Some(idx_ctr) = idx_counter {
            Some(CoreExpr {
                kind: CoreExprKind::Let {
                    local: idx_ctr,
                    value: Box::new(CoreExpr {
                        kind: CoreExprKind::LitInt(0),
                        ty: MonoType::Int,
                        span: for_span,
                    }),
                    body: Box::new(with_iter),
                },
                ty: cont_ty,
                span: for_span,
            })
        } else {
            Some(with_iter)
        }
    }

    // -----------------------------------------------------------------------
    // Range for-loop lowering
    // -----------------------------------------------------------------------

    /// Lower `for x in range_expr { body }` (and indexed form) where range_expr: Range.
    ///
    /// Emits a simple integer counter loop — no array allocation:
    ///   Let(r, range_expr,
    ///     Let(cur, r.start,
    ///       Let(end, r.end,
    ///         Let(step, r.step,
    ///           Loop { If(cur >= end, Break,
    ///                     Let(x, cur, [Let(i, cur,)] body; Assign(cur, cur+step); Continue) }))))
    #[allow(clippy::too_many_arguments)]
    fn lower_range_for_stmt(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
        rest: &[Stmt],
        cont_span: Span,
        for_span: Span,
    ) -> Option<CoreExpr> {
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;
        let range_named_ty = MonoType::named(RANGE_TYPE_ID);

        let r_tmp = self.local_allocator.alloc_and_bind("__r".to_string());
        let cur_tmp = self.local_allocator.alloc_and_bind("__cur".to_string());
        let end_tmp = self.local_allocator.alloc_and_bind("__end".to_string());
        let step_tmp = self.local_allocator.alloc_and_bind("__step".to_string());

        // Field access: r.start (0), r.end (1), r.step (2)
        let start_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(r_tmp),
                    ty: range_named_ty.clone(),
                    span: iter_span,
                }),
                field: FieldId(0),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let end_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(r_tmp),
                    ty: range_named_ty.clone(),
                    span: iter_span,
                }),
                field: FieldId(1),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let step_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(r_tmp),
                    ty: range_named_ty,
                    span: iter_span,
                }),
                field: FieldId(2),
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        let cur_expr = CoreExpr {
            kind: CoreExprKind::Local(cur_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let end_expr = CoreExpr {
            kind: CoreExprKind::Local(end_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let step_expr = CoreExpr {
            kind: CoreExprKind::Local(step_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };

        // Loop body
        self.local_allocator.push_scope();

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };

        // Optional user-visible index variable (separate counter matching cur)
        let idx_user = index_pattern.and_then(|ip| {
            if let Pattern::Ident(name, _) = ip {
                Some(self.local_allocator.alloc_and_bind(name.clone()))
            } else {
                None
            }
        });

        let body_expr = self.lower_block(body)?;
        self.local_allocator.pop_scope();

        // cur = cur + step, then Continue
        let cur_plus_step = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(cur_expr.clone()),
                right: Box::new(step_expr),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let continue_expr = CoreExpr {
            kind: CoreExprKind::Continue,
            ty: MonoType::Void,
            span: iter_span,
        };
        let cur_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: cur_tmp,
                value: Box::new(cur_plus_step),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        // Increment cur BEFORE body so that if body hits `continue`
        // the counter is already advanced to the next element.
        let after_body = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(cur_inc),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: self.local_allocator.alloc(),
                        value: Box::new(body_expr),
                        body: Box::new(continue_expr),
                    },
                    ty: MonoType::Void,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Optionally bind user-visible index before the increment
        let loop_body_inner = if let Some(user_idx) = idx_user {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: user_idx,
                    value: Box::new(cur_expr.clone()),
                    body: Box::new(after_body),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            after_body
        };

        // Let(x, cur, loop_body_inner)
        let elem_let = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(cur_expr.clone()),
                body: Box::new(loop_body_inner),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // If(cur >= end, Break, elem_let)
        let break_expr = CoreExpr {
            kind: CoreExprKind::Break { value: None },
            ty: MonoType::Void,
            span: for_span,
        };
        let guard = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Ge,
                left: Box::new(cur_expr),
                right: Box::new(end_expr),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let if_expr = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(guard),
                then_branch: Box::new(break_expr),
                else_branch: Box::new(elem_let),
            },
            ty: MonoType::Void,
            span: for_span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(if_expr),
            },
            ty: MonoType::Void,
            span: for_span,
        };

        // Add continuation after the loop
        let continuation = self.lower_stmts(rest, cont_span)?;
        let cont_ty = continuation.ty.clone();
        let loop_then_cont = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(loop_expr),
                body: Box::new(continuation),
            },
            ty: cont_ty.clone(),
            span: for_span,
        };

        // Wrap: Let(r, iter, Let(cur, start_get, Let(end, end_get, Let(step, step_get, loop_then_cont))))
        Some(CoreExpr {
            kind: CoreExprKind::Let {
                local: r_tmp,
                value: Box::new(iter_expr),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Let {
                        local: cur_tmp,
                        value: Box::new(start_get),
                        body: Box::new(CoreExpr {
                            kind: CoreExprKind::Let {
                                local: end_tmp,
                                value: Box::new(end_get),
                                body: Box::new(CoreExpr {
                                    kind: CoreExprKind::Let {
                                        local: step_tmp,
                                        value: Box::new(step_get),
                                        body: Box::new(loop_then_cont),
                                    },
                                    ty: cont_ty.clone(),
                                    span: for_span,
                                }),
                            },
                            ty: cont_ty.clone(),
                            span: for_span,
                        }),
                    },
                    ty: cont_ty.clone(),
                    span: for_span,
                }),
            },
            ty: cont_ty,
            span: for_span,
        })
    }

    // -----------------------------------------------------------------------
    // Expression lowering
    // -----------------------------------------------------------------------

    fn lower_expr(&mut self, expr: &Expr) -> Option<CoreExpr> {
        let ty = match self.type_map.get_expr_type(expr.id) {
            Some(t) => t.clone(),
            None => {
                self.errors.push(LowerError::InternalError {
                    message: format!("no type for expr {:?}", expr.id),
                    span: expr.span,
                });
                return None;
            }
        };

        let kind = self.lower_expr_kind(&expr.kind, &ty, expr.span)?;
        Some(CoreExpr {
            kind,
            ty,
            span: expr.span,
        })
    }

    fn lower_index_core_expr(
        &mut self,
        base_expr: CoreExpr,
        index_expr: CoreExpr,
        result_ty: MonoType,
        span: Span,
    ) -> CoreExpr {
        if matches!(base_expr.ty, MonoType::String) {
            // String indexing returns Byte at byte offset (traps on OOB)
            CoreExpr {
                kind: CoreExprKind::Index {
                    base: Box::new(base_expr),
                    index: Box::new(index_expr),
                },
                ty: MonoType::Byte,
                span,
            }
        } else {
            CoreExpr {
                kind: CoreExprKind::Index {
                    base: Box::new(base_expr),
                    index: Box::new(index_expr),
                },
                ty: result_ty,
                span,
            }
        }
    }

    fn lower_expr_kind(
        &mut self,
        kind: &ExprKind,
        ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        match kind {
            // --- Literals ---
            ExprKind::Literal(lit) => Some(match lit {
                Literal::Int(n) => CoreExprKind::LitInt(*n),
                Literal::Float(f) => CoreExprKind::LitFloat(*f),
                Literal::Bool(b) => CoreExprKind::LitBool(*b),
                Literal::String(s) => CoreExprKind::LitStr(s.clone()),
            }),

            // --- Identifier ---
            ExprKind::Ident(name) => {
                if let Some(local_id) = self.local_allocator.lookup(name) {
                    Some(CoreExprKind::Local(local_id))
                } else if self.qualified_func_targets.contains_key(name.as_str())
                    || self.func_table.contains_key(name.as_str())
                {
                    let func_id = self.resolve_named_func_id(name, span)?;
                    Some(CoreExprKind::GlobalFunc(func_id))
                } else if let Some(&global_id) = self.module_globals.get(name.as_str()) {
                    Some(CoreExprKind::GlobalLocal(global_id))
                } else {
                    self.errors.push(LowerError::InternalError {
                        message: format!("unresolved name '{}' during lowering", name),
                        span,
                    });
                    None
                }
            }

            // --- Binary / Unary ---
            ExprKind::Binary { op, left, right } => {
                // Handle assignment expressions (e.g. as case arm bodies).
                if matches!(op, BinOp::Assign) {
                    // Simple ident rebinding: x = expr
                    if let ExprKind::Ident(name) = &left.kind {
                        if let Some(existing_local) = self.local_allocator.lookup(name) {
                            let value_expr = self.lower_expr(right)?;
                            return Some(CoreExprKind::Assign {
                                local: existing_local,
                                value: Box::new(value_expr),
                            });
                        }
                    }
                    // Complex lvalue: r.field = val, arr[i] = val, m[k] = val
                    let rhs_core = self.lower_expr(right)?;
                    if let Some((root_local, update_expr)) = self.lower_lvalue_chain(left, rhs_core)
                    {
                        return Some(CoreExprKind::Assign {
                            local: root_local,
                            value: Box::new(update_expr),
                        });
                    }
                    return None;
                }

                // Preserve short-circuit semantics by lowering logical operators
                // to explicit conditionals instead of eager binary operations.
                if matches!(op, BinOp::And) {
                    let cond = self.lower_expr(left)?;
                    let then_expr = self.lower_expr(right)?;
                    let else_expr = CoreExpr {
                        kind: CoreExprKind::LitBool(false),
                        ty: MonoType::Bool,
                        span,
                    };
                    return Some(CoreExprKind::If {
                        cond: Box::new(cond),
                        then_branch: Box::new(then_expr),
                        else_branch: Box::new(else_expr),
                    });
                }
                if matches!(op, BinOp::Or) {
                    let cond = self.lower_expr(left)?;
                    let then_expr = CoreExpr {
                        kind: CoreExprKind::LitBool(true),
                        ty: MonoType::Bool,
                        span,
                    };
                    let else_expr = self.lower_expr(right)?;
                    return Some(CoreExprKind::If {
                        cond: Box::new(cond),
                        then_branch: Box::new(then_expr),
                        else_branch: Box::new(else_expr),
                    });
                }

                // Desugar range literal: m..n → range_from(m, n)
                if matches!(op, BinOp::Range) {
                    let l = self.lower_expr(left)?;
                    let r = self.lower_expr(right)?;
                    let range_ty = MonoType::named(RANGE_TYPE_ID);
                    let func_expr = CoreExpr {
                        kind: CoreExprKind::GlobalFunc(prelude::RANGE_FROM),
                        ty: MonoType::Function {
                            params: vec![MonoType::Int, MonoType::Int],
                            ret: Box::new(range_ty.clone()),
                        },
                        span,
                    };
                    return Some(CoreExprKind::Call {
                        callee: Box::new(func_expr),
                        args: vec![l, r],
                    });
                }

                let l = self.lower_expr(left)?;
                let r = self.lower_expr(right)?;
                if let Some(rewritten) =
                    lower_unit_variant_compare(*op, &self.type_env, &l, &r, span)
                {
                    return Some(rewritten);
                }
                Some(CoreExprKind::BinOp {
                    op: *op,
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }

            ExprKind::Unary { op, expr: inner } => {
                let e = self.lower_expr(inner)?;
                Some(CoreExprKind::UnOp {
                    op: *op,
                    expr: Box::new(e),
                })
            }

            // --- Function call ---
            ExprKind::Call { callee, args } => self.lower_call(callee, args, ty, span),

            // --- Field access → RecordGet or method call ---
            ExprKind::FieldAccess { base, field } => {
                // Module alias first-class function/value reference: Vector.len, math.pi, etc.
                if let ExprKind::Ident(alias) = &base.kind {
                    if self.can_use_module_alias(alias) {
                        let qualified = format!("{}.{}", alias, field);
                        if self.qualified_func_targets.contains_key(&qualified)
                            || self.func_table.contains_key(&qualified)
                        {
                            let func_id = self.resolve_named_func_id(&qualified, span)?;
                            return Some(CoreExprKind::GlobalFunc(func_id));
                        }
                        if let Some(&local_id) = self.qualified_value_globals.get(&qualified) {
                            return Some(CoreExprKind::GlobalLocal(local_id));
                        }
                        self.errors.push(LowerError::InternalError {
                            message: format!("unknown module reference '{}.{}'", alias, field),
                            span,
                        });
                        return None;
                    }
                }

                // TypeName.Variant or module.TypeName.Variant (zero-arg variant construction)
                if let Some(type_id) = self.try_resolve_type_from_expr(base) {
                    if let Some(variant_idx) = self.type_env.get_variant_index(type_id, field) {
                        return Some(CoreExprKind::Variant {
                            type_id,
                            variant: VariantId(variant_idx),
                            args: vec![],
                        });
                    }
                }

                let base_expr = self.lower_expr(base)?;
                let base_ty = base_expr.ty.clone();

                match &base_ty {
                    MonoType::Named { type_id, .. } => {
                        if let Some(idx) = self.type_env.get_field_index(*type_id, field) {
                            Some(CoreExprKind::RecordGet {
                                target: Box::new(base_expr),
                                field: FieldId(idx),
                            })
                        } else if let Some(func_name) =
                            self.type_env.get_method_function(*type_id, field).cloned()
                        {
                            // First-class method value reference: receiver.method
                            // Lower to a closure that captures the receiver and calls the method.
                            self.lower_method_value_ref(base_expr, &func_name, ty, span)
                        } else {
                            self.errors.push(LowerError::UnknownField {
                                field: field.clone(),
                                type_name: format!("Type#{}", type_id.0),
                                span,
                            });
                            None
                        }
                    }
                    _ => {
                        // Method value reference resolved through TypeEnv/ValueEnv
                        // (covers both builtin receiver types and named types).
                        let error_count_before = self.errors.len();
                        if let Some(func_id) =
                            self.resolve_registered_method_func_id(&base_ty, field, span)
                        {
                            self.lower_builtin_method_value_ref(base_expr, func_id, ty, span)
                        } else if self.errors.len() > error_count_before {
                            None
                        } else {
                            self.errors.push(LowerError::InternalError {
                                message: format!("field access on non-record type {:?}", base_ty),
                                span,
                            });
                            None
                        }
                    }
                }
            }

            // --- Index ---
            ExprKind::Index { base, index } => {
                let base_ty = self.type_map.get_expr_type(base.id).cloned();
                let base_expr = self.lower_expr(base)?;
                let index_expr = self.lower_expr(index)?;

                // Dict[key] lowers to dict_get(dict, key) → Option<V>
                if matches!(base_ty, Some(MonoType::Dict(_, _))) {
                    let func_ty = MonoType::Function {
                        params: vec![base_expr.ty.clone(), index_expr.ty.clone()],
                        ret: Box::new(base_expr.ty.clone()), // placeholder
                    };
                    let func_expr = CoreExpr {
                        kind: CoreExprKind::GlobalFunc(prelude::DICT_GET),
                        ty: func_ty,
                        span,
                    };
                    Some(CoreExprKind::Call {
                        callee: Box::new(func_expr),
                        args: vec![base_expr, index_expr],
                    })
                } else {
                    let result_ty = match base_ty {
                        Some(MonoType::Vector(inner)) => *inner,
                        Some(MonoType::String) => MonoType::String,
                        _ => MonoType::Void,
                    };
                    Some(
                        self.lower_index_core_expr(base_expr, index_expr, result_ty, span)
                            .kind,
                    )
                }
            }

            // --- Array literal ---
            ExprKind::Array { elements } => {
                let mut lowered = Vec::new();
                for e in elements {
                    lowered.push(self.lower_expr(e)?);
                }
                Some(CoreExprKind::ArrayLit { elements: lowered })
            }

            // --- If expression ---
            ExprKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_expr = self.lower_expr(cond)?;
                let then_expr = self.lower_expr(then_branch)?;
                let else_expr = match else_branch {
                    Some(e) => self.lower_expr(e)?,
                    None => CoreExpr {
                        kind: CoreExprKind::LitVoid,
                        ty: MonoType::Void,
                        span,
                    },
                };
                Some(CoreExprKind::If {
                    cond: Box::new(cond_expr),
                    then_branch: Box::new(then_expr),
                    else_branch: Box::new(else_expr),
                })
            }

            // --- Block expression ---
            ExprKind::Block(block) => {
                self.local_allocator.push_scope();
                let result = self.lower_block(block)?;
                self.local_allocator.pop_scope();
                // Unwrap the inner block's kind; the ty is already set by lower_expr
                Some(result.kind)
            }

            // --- Record literal ---
            ExprKind::RecordLit { name: _, fields } => {
                let type_id = match ty {
                    MonoType::Named { type_id, .. } => *type_id,
                    _ => {
                        self.errors
                            .push(LowerError::RecordNeedsTypeContext { span });
                        return None;
                    }
                };

                let mut lowered_fields = Vec::new();
                for (field_name, field_expr) in fields {
                    let idx = match self.type_env.get_field_index(type_id, field_name) {
                        Some(i) => i,
                        None => {
                            self.errors.push(LowerError::UnknownField {
                                field: field_name.clone(),
                                type_name: format!("Type#{}", type_id.0),
                                span,
                            });
                            return None;
                        }
                    };
                    let lowered_val = self.lower_expr(field_expr)?;
                    lowered_fields.push((FieldId(idx), lowered_val));
                }
                // Sort by FieldId for deterministic output
                lowered_fields.sort_by_key(|(fid, _)| *fid);
                Some(CoreExprKind::Record {
                    type_id,
                    fields: lowered_fields,
                })
            }

            // --- Variant literal ---
            ExprKind::VariantLit {
                name: variant_name,
                fields,
            } => {
                let type_id = match ty {
                    MonoType::Named { type_id, .. } => *type_id,
                    _ => {
                        self.errors
                            .push(LowerError::VariantNeedsTypeContext { span });
                        return None;
                    }
                };

                let variant_idx = match self.type_env.get_variant_index(type_id, variant_name) {
                    Some(i) => i,
                    None => {
                        self.errors.push(LowerError::UnknownVariant {
                            name: variant_name.clone(),
                            type_name: format!("Type#{}", type_id.0),
                            span,
                        });
                        return None;
                    }
                };

                let mut lowered_args = Vec::new();
                for f in fields {
                    lowered_args.push(self.lower_expr(f)?);
                }

                Some(CoreExprKind::Variant {
                    type_id,
                    variant: VariantId(variant_idx),
                    args: lowered_args,
                })
            }

            // --- Case expression → Match ---
            ExprKind::Case { scrutinee, arms } => {
                let scrut_expr = self.lower_expr(scrutinee)?;
                let scrut_ty = scrut_expr.ty.clone();
                let mut lowered_arms = Vec::new();
                for arm in arms {
                    if let Some(la) = self.lower_case_arm(arm, &scrut_ty) {
                        lowered_arms.push(la);
                    } else {
                        return None;
                    }
                }
                Some(CoreExprKind::Match {
                    scrutinee: Box::new(scrut_expr),
                    arms: lowered_arms,
                })
            }

            // --- String interpolation ---
            ExprKind::StringInterpolation { parts } => self.lower_string_interpolation(parts, span),

            // --- Collect expression ---
            ExprKind::Collect {
                pattern,
                index_pattern,
                iter,
                body,
            } => self.lower_collect(pattern, index_pattern.as_ref(), iter, body, ty, span),
            ExprKind::CollectWhile { cond, body } => self.lower_collect_while(cond, body, ty, span),

            // --- Lambda / closure ---
            ExprKind::Function(fe) => {
                // Bind lambda params in a new scope using the shared allocator
                // (so LocalIds are unique across the enclosing function and lambda).
                self.local_allocator.push_scope();
                let lambda_params: Vec<LocalId> = fe
                    .params
                    .iter()
                    .map(|p| self.local_allocator.alloc_and_bind(p.name.clone()))
                    .collect();

                // Save/restore current_fn_return_type for the lambda scope
                let saved_ret = self.current_fn_return_type.take();
                let lambda_ret = if let MonoType::Function { ret, .. } = ty {
                    Some(ret.as_ref().clone())
                } else {
                    None
                };
                self.current_fn_return_type = lambda_ret;

                let body = self.lower_expr(&fe.body)?;
                self.current_fn_return_type = saved_ret;
                self.local_allocator.pop_scope();

                // Collect free variables: Local(id) refs not in lambda params
                let param_set: HashSet<LocalId> = lambda_params.iter().copied().collect();
                let free_vars = collect_local_refs(&body, &param_set);

                // Hoist to a new FunctionDef
                let func_id = self.alloc_hoisted_id();
                let return_ty = body.ty.clone();
                // Extract lambda param types from the expression's Function type
                let lambda_param_tys = if let MonoType::Function { params, .. } = ty {
                    params.clone()
                } else {
                    vec![]
                };
                let hoisted = FunctionDef {
                    func_id,
                    name: format!("<lambda@{}>", span.start),
                    params: lambda_params,
                    param_tys: lambda_param_tys,
                    body,
                    return_ty,
                };
                self.hoisted_functions.push(hoisted);

                Some(CoreExprKind::MakeClosure { func_id, free_vars })
            }

            // --- Try (deferred until generics) ---
            ExprKind::Try { expr: inner_expr } => {
                let inner = self.lower_expr(inner_expr)?;
                let inner_ty = inner.ty.clone();

                let is_option = matches!(
                    &inner_ty,
                    MonoType::Named { type_id, .. } if *type_id == OPTION_TYPE_ID
                );

                if is_option {
                    // try Option<T> desugars to:
                    //   let tmp = expr
                    //   match tmp {
                    //     .Some(v) => v,
                    //     .None    => return .None,
                    //   }
                    let tmp_local = self.local_allocator.alloc();
                    let v_local = self.local_allocator.alloc();

                    let payload_ty = match &inner_ty {
                        MonoType::Named { args, .. } => {
                            args.first().cloned().unwrap_or(MonoType::Void)
                        }
                        _ => MonoType::Void,
                    };

                    // Some arm: extract payload (Some = VariantId(1))
                    let some_pattern = CorePattern::Variant {
                        type_id: OPTION_TYPE_ID,
                        variant: VariantId(1),
                        fields: vec![CorePattern::Var(v_local)],
                    };
                    let some_body = CoreExpr {
                        kind: CoreExprKind::Local(v_local),
                        ty: payload_ty.clone(),
                        span,
                    };

                    // None arm: return None
                    // Use the enclosing function's return type for the None variant
                    let ret_ty = self
                        .current_fn_return_type
                        .clone()
                        .unwrap_or(inner_ty.clone());
                    let none_variant = CoreExpr {
                        kind: CoreExprKind::Variant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(0), // None = VariantId(0)
                            args: vec![],
                        },
                        ty: ret_ty,
                        span,
                    };
                    let none_body = CoreExpr {
                        kind: CoreExprKind::Return {
                            value: Some(Box::new(none_variant)),
                        },
                        ty: MonoType::Never,
                        span,
                    };
                    let none_pattern = CorePattern::Variant {
                        type_id: OPTION_TYPE_ID,
                        variant: VariantId(0), // None = VariantId(0)
                        fields: vec![],
                    };

                    let match_expr = CoreExpr {
                        kind: CoreExprKind::Match {
                            scrutinee: Box::new(CoreExpr {
                                kind: CoreExprKind::Local(tmp_local),
                                ty: inner_ty.clone(),
                                span,
                            }),
                            arms: vec![
                                MatchArm {
                                    pattern: some_pattern,
                                    body: some_body,
                                },
                                MatchArm {
                                    pattern: none_pattern,
                                    body: none_body,
                                },
                            ],
                        },
                        ty: payload_ty,
                        span,
                    };

                    Some(CoreExprKind::Let {
                        local: tmp_local,
                        value: Box::new(inner),
                        body: Box::new(match_expr),
                    })
                } else {
                    // try Result<T,E> desugars to:
                    //   let tmp = expr
                    //   match tmp {
                    //     .Ok(v)  => v,
                    //     .Err(e) => return .Err(e),
                    //   }
                    let tmp_local = self.local_allocator.alloc();
                    let v_local = self.local_allocator.alloc();
                    let e_local = self.local_allocator.alloc();

                    let ok_ty = match &inner_ty {
                        MonoType::Named { args, .. } => {
                            args.first().cloned().unwrap_or(MonoType::Void)
                        }
                        _ => MonoType::Void,
                    };
                    let err_ty = match &inner_ty {
                        MonoType::Named { args, .. } => {
                            args.get(1).cloned().unwrap_or(MonoType::Void)
                        }
                        _ => MonoType::Void,
                    };

                    let ok_pattern = CorePattern::Variant {
                        type_id: RESULT_TYPE_ID,
                        variant: VariantId(0),
                        fields: vec![CorePattern::Var(v_local)],
                    };
                    let ok_body = CoreExpr {
                        kind: CoreExprKind::Local(v_local),
                        ty: ok_ty.clone(),
                        span,
                    };

                    let err_payload = CoreExpr {
                        kind: CoreExprKind::Local(e_local),
                        ty: err_ty.clone(),
                        span,
                    };
                    let err_variant = CoreExpr {
                        kind: CoreExprKind::Variant {
                            type_id: RESULT_TYPE_ID,
                            variant: VariantId(1),
                            args: vec![err_payload],
                        },
                        ty: inner_ty.clone(),
                        span,
                    };
                    let err_body = CoreExpr {
                        kind: CoreExprKind::Return {
                            value: Some(Box::new(err_variant)),
                        },
                        ty: MonoType::Never,
                        span,
                    };

                    let err_pattern = CorePattern::Variant {
                        type_id: RESULT_TYPE_ID,
                        variant: VariantId(1),
                        fields: vec![CorePattern::Var(e_local)],
                    };

                    let match_expr = CoreExpr {
                        kind: CoreExprKind::Match {
                            scrutinee: Box::new(CoreExpr {
                                kind: CoreExprKind::Local(tmp_local),
                                ty: inner_ty.clone(),
                                span,
                            }),
                            arms: vec![
                                MatchArm {
                                    pattern: ok_pattern,
                                    body: ok_body,
                                },
                                MatchArm {
                                    pattern: err_pattern,
                                    body: err_body,
                                },
                            ],
                        },
                        ty: ok_ty,
                        span,
                    };

                    Some(CoreExprKind::Let {
                        local: tmp_local,
                        value: Box::new(inner),
                        body: Box::new(match_expr),
                    })
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Call lowering
    // -----------------------------------------------------------------------

    fn lower_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        ret_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        // Field-access calls: module.func(args) or receiver.method(args)
        if let ExprKind::FieldAccess { base, field } = &callee.kind {
            // Module-qualified call: alias.func(args) — check before constructor
            // to match synth_call resolution order in the type checker.
            if let ExprKind::Ident(alias) = &base.kind {
                if self.can_use_module_alias(alias) {
                    let qualified = format!("{}.{}", alias, field);
                    if self.qualified_func_targets.contains_key(&qualified)
                        || self.func_table.contains_key(&qualified)
                    {
                        let func_id = self.resolve_named_func_id(&qualified, span)?;
                        let mut lowered_args = Vec::new();
                        for a in args {
                            lowered_args.push(self.lower_expr(a)?);
                        }
                        let func_ty = MonoType::Function {
                            params: lowered_args.iter().map(|a| a.ty.clone()).collect(),
                            ret: Box::new(ret_ty.clone()),
                        };
                        let func_expr = CoreExpr {
                            kind: CoreExprKind::GlobalFunc(func_id),
                            ty: func_ty,
                            span,
                        };
                        return Some(CoreExprKind::Call {
                            callee: Box::new(func_expr),
                            args: lowered_args,
                        });
                    }
                    self.errors.push(LowerError::InternalError {
                        message: format!("no FuncId for '{}'", qualified),
                        span,
                    });
                    return None;
                }
            }

            // TypeName.Variant(args) or module.TypeName.Variant(args)
            if let Some(type_id) = self.try_resolve_type_from_expr(base) {
                if let Some(variant_idx) = self.type_env.get_variant_index(type_id, field) {
                    let mut lowered_args = Vec::new();
                    for a in args {
                        lowered_args.push(self.lower_expr(a)?);
                    }
                    return Some(CoreExprKind::Variant {
                        type_id,
                        variant: VariantId(variant_idx),
                        args: lowered_args,
                    });
                }
            }

            // Method call via FieldAccess: receiver.method(args)
            return self.lower_method_call(base, field, args, ret_ty, span);
        }

        // Regular call
        let callee_expr = self.lower_expr(callee)?;
        let mut lowered_args = Vec::new();
        for a in args {
            lowered_args.push(self.lower_expr(a)?);
        }
        Some(CoreExprKind::Call {
            callee: Box::new(callee_expr),
            args: lowered_args,
        })
    }

    fn lower_method_call(
        &mut self,
        base: &Expr,
        method: &str,
        args: &[Expr],
        _ret_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let base_expr = self.lower_expr(base)?;
        let base_ty = base_expr.ty.clone();

        let mut all_args = vec![base_expr];
        for a in args {
            all_args.push(self.lower_expr(a)?);
        }

        let error_count_before = self.errors.len();
        let func_id = if let Some(func_id) =
            self.resolve_registered_method_func_id(&base_ty, method, span)
        {
            func_id
        } else if matches!(&base_ty, MonoType::Var(name) if method == "to_string" && self.current_type_param_bounds.get(name).map(|b| b.as_str()) == Some("Stringify"))
        {
            return Some(CoreExprKind::ContractCall {
                contract: "Stringify".to_string(),
                method: method.to_string(),
                receiver: Box::new(all_args.remove(0)),
                args: all_args,
            });
        } else if self.errors.len() > error_count_before {
            return None;
        } else if let MonoType::Named { type_id, .. } = base_ty.clone() {
            if let Some(field_idx) = self.type_env.get_field_index(type_id, method) {
                // Function-typed record field: `record.fn_field(args)` — call the closure stored in the field
                let field_ty = self
                    .type_env
                    .get_record_fields(type_id)
                    .and_then(|fields| fields.get(field_idx))
                    .map(|f| f.ty.clone())
                    .unwrap_or(MonoType::Void);
                let field_expr = CoreExpr {
                    kind: CoreExprKind::RecordGet {
                        target: Box::new(all_args[0].clone()),
                        field: crate::ir::core::FieldId(field_idx),
                    },
                    ty: field_ty,
                    span,
                };
                return Some(CoreExprKind::Call {
                    callee: Box::new(field_expr),
                    args: all_args.into_iter().skip(1).collect(),
                });
            }
            self.errors.push(LowerError::InternalError {
                message: format!(
                    "no inherent method '{}' for type Type#{}",
                    method, type_id.0
                ),
                span,
            });
            return None;
        } else {
            self.errors.push(LowerError::InternalError {
                message: format!("no inherent method '{}' for type {:?}", method, base_ty),
                span,
            });
            return None;
        };

        let func_ty = MonoType::Function {
            params: all_args.iter().map(|a| a.ty.clone()).collect(),
            ret: Box::new(MonoType::Int), // placeholder; actual ty set by lower_expr wrapper
        };

        let func_expr = CoreExpr {
            kind: CoreExprKind::GlobalFunc(func_id),
            ty: func_ty,
            span,
        };

        Some(CoreExprKind::Call {
            callee: Box::new(func_expr),
            args: all_args,
        })
    }

    // -----------------------------------------------------------------------
    // First-class method value references
    // -----------------------------------------------------------------------

    /// Lower `receiver.method` to a closure that captures the receiver and calls the method.
    /// `base_expr` is the already-lowered receiver expression.
    /// `func_name` is the qualified method function name.
    /// `result_ty` is the function type with receiver stripped (from type checker).
    fn lower_method_value_ref(
        &mut self,
        base_expr: CoreExpr,
        func_name: &str,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let method_func_id = self.resolve_named_func_id(func_name, span)?;
        self.lower_method_value_ref_with_id(base_expr, method_func_id, result_ty, span)
    }

    /// Lower a builtin method value reference (e.g. xs.len, n.to_string).
    fn lower_builtin_method_value_ref(
        &mut self,
        base_expr: CoreExpr,
        func_id: FuncId,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        self.lower_method_value_ref_with_id(base_expr, func_id, result_ty, span)
    }

    /// Core: lower a method value reference given a resolved FuncId.
    fn lower_method_value_ref_with_id(
        &mut self,
        base_expr: CoreExpr,
        method_func_id: FuncId,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        // Extract param types and return type from the result function type
        let (param_tys, return_ty) = match result_ty {
            MonoType::Function { params, ret } => (params.clone(), (**ret).clone()),
            _ => {
                self.errors.push(LowerError::InternalError {
                    message: "method value ref has non-function type".to_string(),
                    span,
                });
                return None;
            }
        };

        // Store receiver in a local so it's evaluated once
        let recv_local = self
            .local_allocator
            .alloc_and_bind(format!("__mref_recv@{}", span.start));
        let recv_ty = base_expr.ty.clone();

        // Create params for the wrapper function (one per non-receiver arg)
        self.local_allocator.push_scope();
        let wrapper_params: Vec<LocalId> = param_tys
            .iter()
            .enumerate()
            .map(|(i, _)| {
                self.local_allocator
                    .alloc_and_bind(format!("__mref_arg{}", i))
            })
            .collect();

        // Build call: method_func(captured_receiver, arg0, arg1, ...)
        let mut call_args = Vec::with_capacity(1 + wrapper_params.len());
        call_args.push(CoreExpr {
            kind: CoreExprKind::Local(recv_local),
            ty: recv_ty.clone(),
            span,
        });
        for (i, param_id) in wrapper_params.iter().enumerate() {
            call_args.push(CoreExpr {
                kind: CoreExprKind::Local(*param_id),
                ty: param_tys[i].clone(),
                span,
            });
        }

        let all_param_tys: Vec<MonoType> = std::iter::once(recv_ty.clone())
            .chain(param_tys.iter().cloned())
            .collect();
        let func_expr = CoreExpr {
            kind: CoreExprKind::GlobalFunc(method_func_id),
            ty: MonoType::Function {
                params: all_param_tys,
                ret: Box::new(return_ty.clone()),
            },
            span,
        };
        let body = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(func_expr),
                args: call_args,
            },
            ty: return_ty.clone(),
            span,
        };

        self.local_allocator.pop_scope();

        // Hoist wrapper as a FunctionDef
        let func_id = self.alloc_hoisted_id();
        let hoisted = FunctionDef {
            func_id,
            name: format!("<method_ref@{}>", span.start),
            params: wrapper_params.clone(),
            param_tys: param_tys.clone(),
            body,
            return_ty,
        };
        self.hoisted_functions.push(hoisted);

        // Emit Let(recv_local, base_expr, MakeClosure { func_id, free_vars: [recv_local] })
        let closure = CoreExpr {
            kind: CoreExprKind::MakeClosure {
                func_id,
                free_vars: vec![recv_local],
            },
            ty: result_ty.clone(),
            span,
        };
        Some(CoreExprKind::Let {
            local: recv_local,
            value: Box::new(base_expr),
            body: Box::new(closure),
        })
    }

    // -----------------------------------------------------------------------
    // Case arm / pattern lowering
    // -----------------------------------------------------------------------

    fn lower_case_arm(&mut self, arm: &CaseArm, scrut_ty: &MonoType) -> Option<MatchArm> {
        self.local_allocator.push_scope();
        let pattern = self.lower_pattern(&arm.pattern, Some(scrut_ty))?;
        let body = self.lower_expr(&arm.body)?;
        self.local_allocator.pop_scope();
        Some(MatchArm { pattern, body })
    }

    /// Lower a pattern, using `scrutinee_ty` (when known) to resolve variant TypeIds
    /// without an ambiguous name scan across all registered types.
    fn lower_pattern(
        &mut self,
        pattern: &Pattern,
        scrutinee_ty: Option<&MonoType>,
    ) -> Option<CorePattern> {
        match pattern {
            Pattern::Wildcard(_) => Some(CorePattern::Wildcard),

            Pattern::Ident(name, _) => {
                let local = self.local_allocator.alloc_and_bind(name.clone());
                Some(CorePattern::Var(local))
            }

            Pattern::Literal(lit, span) => Some(match lit {
                Literal::Int(n) => CorePattern::LitInt(*n),
                Literal::Bool(b) => CorePattern::LitBool(*b),
                Literal::String(s) => CorePattern::LitStr(s.clone()),
                Literal::Float(_) => {
                    self.errors.push(LowerError::UnsupportedFeature {
                        feature: "float pattern matching",
                        span: *span,
                    });
                    return None;
                }
            }),

            Pattern::Variant {
                type_name: qual_type_name,
                name: variant_name,
                fields,
                span: pat_span,
            } => {
                // Resolve TypeId using three strategies in priority order:
                // 1. Qualified type name lookup (TypeName.Variant form) — fast, direct.
                //    Note: bare type names are removed after per-module typecheck in multi-module
                //    builds, so this may return None even for valid qualified patterns.
                // 2. Scrutinee's declared MonoType — correct even after name removal; the
                //    type checker has already validated that the variant belongs to this type.
                // 3. Full scan — last resort when scrutinee type is unknown.
                let resolved: Option<(crate::types::ty::TypeId, usize)> = 'resolve: {
                    if let Some(tname) = qual_type_name {
                        if let Some(tid) = self.type_env.lookup_type(tname) {
                            if let Some(idx) = self.type_env.get_variant_index(tid, variant_name) {
                                break 'resolve Some((tid, idx));
                            }
                        }
                    }
                    if let Some(MonoType::Named { type_id, .. }) = scrutinee_ty {
                        if let Some(idx) = self.type_env.get_variant_index(*type_id, variant_name) {
                            break 'resolve Some((*type_id, idx));
                        }
                    }
                    let mut found = None;
                    for i in 0..self.type_env.type_count() {
                        let tid = crate::types::ty::TypeId(i as u32);
                        if let Some(idx) = self.type_env.get_variant_index(tid, variant_name) {
                            found = Some((tid, idx));
                            break;
                        }
                    }
                    found
                };

                let (type_id, variant_idx) = match resolved {
                    Some(pair) => pair,
                    None => {
                        let tname = qual_type_name.as_deref().unwrap_or("(unknown)");
                        self.errors.push(LowerError::UnknownVariant {
                            name: variant_name.to_string(),
                            type_name: tname.to_string(),
                            span: *pat_span,
                        });
                        return None;
                    }
                };

                // Pass each field pattern the corresponding declared field type so
                // nested variant patterns (e.g. `.Some(.Ok(x))`) also resolve correctly.
                let field_types: Vec<MonoType> = self
                    .type_env
                    .get_variants(type_id)
                    .and_then(|vs| vs.get(variant_idx))
                    .map(|v| v.fields.clone())
                    .unwrap_or_default();

                let mut lowered_fields = Vec::new();
                for (f, field_ty) in fields.iter().zip(&field_types) {
                    lowered_fields.push(self.lower_pattern(f, Some(field_ty))?);
                }
                // Safety net for fields beyond declared count (caught by type-checker)
                for f in fields.iter().skip(field_types.len()) {
                    lowered_fields.push(self.lower_pattern(f, None)?);
                }

                Some(CorePattern::Variant {
                    type_id,
                    variant: VariantId(variant_idx),
                    fields: lowered_fields,
                })
            }
        }
    }

    // -----------------------------------------------------------------------
    // String interpolation
    // -----------------------------------------------------------------------

    fn lower_string_interpolation(
        &mut self,
        parts: &[StringPart],
        span: Span,
    ) -> Option<CoreExprKind> {
        // Right-fold the parts into nested concat calls
        // "a${x}b" → concat("a", concat(to_string(x), "b"))
        let exprs: Vec<CoreExpr> = parts
            .iter()
            .map(|part| match part {
                StringPart::Literal(s) => Some(CoreExpr {
                    kind: CoreExprKind::LitStr(s.clone()),
                    ty: MonoType::String,
                    span,
                }),
                StringPart::Interpolation(e) => {
                    let to_string_call =
                        self.lower_method_call(e, "to_string", &[], &MonoType::String, e.span)?;
                    Some(CoreExpr {
                        kind: to_string_call,
                        ty: MonoType::String,
                        span: e.span,
                    })
                }
            })
            .collect::<Option<Vec<_>>>()?;

        if exprs.is_empty() {
            return Some(CoreExprKind::LitStr(String::new()));
        }

        // Right-fold with concat
        let result = exprs.into_iter().rev().reduce(|right, left| {
            let concat_func = CoreExpr {
                kind: CoreExprKind::GlobalFunc(prelude::STRING_CONCAT),
                ty: MonoType::Function {
                    params: vec![MonoType::String, MonoType::String],
                    ret: Box::new(MonoType::String),
                },
                span,
            };
            CoreExpr {
                kind: CoreExprKind::Call {
                    callee: Box::new(concat_func),
                    args: vec![left, right],
                },
                ty: MonoType::String,
                span,
            }
        })?;

        Some(result.kind)
    }

    // -----------------------------------------------------------------------
    // Collect expression desugaring
    // -----------------------------------------------------------------------

    fn lower_range_collect(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        // Stage 10.1: range-collect specialization.
        //
        // Preallocate once with Vector.make(len, void), fill in-place by index,
        // and trim once at the end with Vector.slice(0, idx). This removes the
        // O(N^2) append/concat behavior while preserving `continue` filtering.

        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;
        let range_named_ty = MonoType::named(RANGE_TYPE_ID);

        let r_tmp = self.local_allocator.alloc_and_bind("__rc_r".to_string());
        let cur_tmp = self.local_allocator.alloc_and_bind("__rc_cur".to_string());
        let end_tmp = self.local_allocator.alloc_and_bind("__rc_end".to_string());
        let step_tmp = self.local_allocator.alloc_and_bind("__rc_step".to_string());
        let len_local = self.local_allocator.alloc_and_bind("__rc_len".to_string());
        let idx_local = self.local_allocator.alloc_and_bind("__rc_idx".to_string());
        let iter_idx_local = self.local_allocator.alloc_and_bind("__rc_i".to_string());
        let acc_local = self.local_allocator.alloc_and_bind("__rc_acc".to_string());

        // Field access: r.start (0), r.end (1), r.step (2)
        let start_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(r_tmp),
                    ty: range_named_ty.clone(),
                    span: iter_span,
                }),
                field: FieldId(0),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let end_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(r_tmp),
                    ty: range_named_ty.clone(),
                    span: iter_span,
                }),
                field: FieldId(1),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let step_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(r_tmp),
                    ty: range_named_ty,
                    span: iter_span,
                }),
                field: FieldId(2),
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        let cur_expr = CoreExpr {
            kind: CoreExprKind::Local(cur_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let end_expr = CoreExpr {
            kind: CoreExprKind::Local(end_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let step_expr = CoreExpr {
            kind: CoreExprKind::Local(step_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_expr = CoreExpr {
            kind: CoreExprKind::Local(idx_local),
            ty: MonoType::Int,
            span: iter_span,
        };
        let iter_idx_expr = CoreExpr {
            kind: CoreExprKind::Local(iter_idx_local),
            ty: MonoType::Int,
            span: iter_span,
        };
        let acc_expr = CoreExpr {
            kind: CoreExprKind::Local(acc_local),
            ty: result_ty.clone(),
            span: iter_span,
        };

        // done = if step > 0 { cur >= end } else if step < 0 { cur <= end } else true
        let step_gt_zero = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Gt,
                left: Box::new(step_expr.clone()),
                right: Box::new(CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let step_lt_zero = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Lt,
                left: Box::new(step_expr.clone()),
                right: Box::new(CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let pos_done = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Ge,
                left: Box::new(cur_expr.clone()),
                right: Box::new(end_expr.clone()),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let neg_done = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Le,
                left: Box::new(cur_expr.clone()),
                right: Box::new(end_expr.clone()),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let done_cond = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(step_gt_zero.clone()),
                then_branch: Box::new(pos_done),
                else_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::If {
                        cond: Box::new(step_lt_zero.clone()),
                        then_branch: Box::new(neg_done),
                        else_branch: Box::new(CoreExpr {
                            kind: CoreExprKind::LitBool(true),
                            ty: MonoType::Bool,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Bool,
                    span: iter_span,
                }),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };

        // len =
        //   if step > 0 {
        //     if cur >= end { 0 } else { (end - cur + (step - 1)) / step }
        //   } else if step < 0 {
        //     abs = 0 - step
        //     if cur <= end { 0 } else { (cur - end + (abs - 1)) / abs }
        //   } else { 0 }
        let pos_len = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Ge,
                        left: Box::new(cur_expr.clone()),
                        right: Box::new(end_expr.clone()),
                    },
                    ty: MonoType::Bool,
                    span: iter_span,
                }),
                then_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
                else_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Div,
                        left: Box::new(CoreExpr {
                            kind: CoreExprKind::BinOp {
                                op: BinOp::Add,
                                left: Box::new(CoreExpr {
                                    kind: CoreExprKind::BinOp {
                                        op: BinOp::Sub,
                                        left: Box::new(end_expr.clone()),
                                        right: Box::new(cur_expr.clone()),
                                    },
                                    ty: MonoType::Int,
                                    span: iter_span,
                                }),
                                right: Box::new(CoreExpr {
                                    kind: CoreExprKind::BinOp {
                                        op: BinOp::Sub,
                                        left: Box::new(step_expr.clone()),
                                        right: Box::new(CoreExpr {
                                            kind: CoreExprKind::LitInt(1),
                                            ty: MonoType::Int,
                                            span: iter_span,
                                        }),
                                    },
                                    ty: MonoType::Int,
                                    span: iter_span,
                                }),
                            },
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                        right: Box::new(step_expr.clone()),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let abs_step = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Sub,
                left: Box::new(CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
                right: Box::new(step_expr.clone()),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let neg_len = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Le,
                        left: Box::new(cur_expr.clone()),
                        right: Box::new(end_expr.clone()),
                    },
                    ty: MonoType::Bool,
                    span: iter_span,
                }),
                then_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::LitInt(0),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
                else_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Div,
                        left: Box::new(CoreExpr {
                            kind: CoreExprKind::BinOp {
                                op: BinOp::Add,
                                left: Box::new(CoreExpr {
                                    kind: CoreExprKind::BinOp {
                                        op: BinOp::Sub,
                                        left: Box::new(cur_expr.clone()),
                                        right: Box::new(end_expr.clone()),
                                    },
                                    ty: MonoType::Int,
                                    span: iter_span,
                                }),
                                right: Box::new(CoreExpr {
                                    kind: CoreExprKind::BinOp {
                                        op: BinOp::Sub,
                                        left: Box::new(abs_step.clone()),
                                        right: Box::new(CoreExpr {
                                            kind: CoreExprKind::LitInt(1),
                                            ty: MonoType::Int,
                                            span: iter_span,
                                        }),
                                    },
                                    ty: MonoType::Int,
                                    span: iter_span,
                                }),
                            },
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                        right: Box::new(abs_step.clone()),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let len_expr = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(step_gt_zero),
                then_branch: Box::new(pos_len),
                else_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::If {
                        cond: Box::new(step_lt_zero),
                        then_branch: Box::new(neg_len),
                        else_branch: Box::new(CoreExpr {
                            kind: CoreExprKind::LitInt(0),
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        self.local_allocator.push_scope();

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };
        let idx_user = index_pattern.and_then(|ip| match ip {
            Pattern::Ident(name, _) => Some(self.local_allocator.alloc_and_bind(name.clone())),
            _ => None,
        });

        let body_val_local = self.local_allocator.alloc_and_bind("__rc_val".to_string());
        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();

        self.local_allocator.pop_scope();

        // cur = cur + step
        let cur_plus_step = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(cur_expr.clone()),
                right: Box::new(step_expr),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let cur_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: cur_tmp,
                value: Box::new(cur_plus_step),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // acc = __vector_set_in_place(acc, idx, val)
        let set_in_place_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(prelude::VECTOR_SET_IN_PLACE),
            ty: MonoType::Function {
                params: vec![result_ty.clone(), MonoType::Int, body_ty.clone()],
                ret: Box::new(result_ty.clone()),
            },
            span: iter_span,
        };
        let acc_new_val = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(set_in_place_func),
                args: vec![
                    acc_expr.clone(),
                    idx_expr.clone(),
                    CoreExpr {
                        kind: CoreExprKind::Local(body_val_local),
                        ty: body_ty,
                        span: iter_span,
                    },
                ],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let acc_assign = CoreExpr {
            kind: CoreExprKind::Assign {
                local: acc_local,
                value: Box::new(acc_new_val),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let idx_plus_one = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(idx_expr.clone()),
                right: Box::new(CoreExpr {
                    kind: CoreExprKind::LitInt(1),
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: idx_local,
                value: Box::new(idx_plus_one),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let iter_idx_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: iter_idx_local,
                value: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Add,
                        left: Box::new(iter_idx_expr.clone()),
                        right: Box::new(CoreExpr {
                            kind: CoreExprKind::LitInt(1),
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let continue_expr = CoreExpr {
            kind: CoreExprKind::Continue,
            ty: MonoType::Void,
            span: iter_span,
        };

        // Build from inside out:
        // Let(_, acc_assign, Let(_, idx_inc, Continue))
        let tmp_idx = self.local_allocator.alloc();
        let after_idx_inc = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp_idx,
                value: Box::new(idx_inc),
                body: Box::new(continue_expr),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let tmp_set = self.local_allocator.alloc();
        let after_set = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp_set,
                value: Box::new(acc_assign),
                body: Box::new(after_idx_inc),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        // Let(val, body, after_set)
        let with_val = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(after_set),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        // Let(_, cur_inc, Let(_, iter_idx_inc, with_val)) — increment BEFORE body so `continue` advances.
        let tmp_i = self.local_allocator.alloc();
        let with_i_inc = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp_i,
                value: Box::new(iter_idx_inc),
                body: Box::new(with_val),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let tmp2 = self.local_allocator.alloc();
        let with_inc = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp2,
                value: Box::new(cur_inc),
                body: Box::new(with_i_inc),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let with_idx_binding = if let Some(idx_local_user) = idx_user {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: idx_local_user,
                    value: Box::new(iter_idx_expr.clone()),
                    body: Box::new(with_inc),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            with_inc
        };
        // Let(x, cur, with_inc)
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(cur_expr.clone()),
                body: Box::new(with_idx_binding),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Break condition: done_cond (handles positive/negative/zero step).
        // Break with the current accumulator; we compact to [0, idx) after loop.
        let break_acc = CoreExpr {
            kind: CoreExprKind::Break {
                value: Some(Box::new(acc_expr.clone())),
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let loop_if = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(done_cond),
                then_branch: Box::new(break_acc),
                else_branch: Box::new(with_elem),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_if),
            },
            ty: result_ty.clone(),
            span,
        };
        let slice_after_loop = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_SLICE),
                    ty: MonoType::Function {
                        params: vec![result_ty.clone(), MonoType::Int, MonoType::Int],
                        ret: Box::new(result_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![
                    acc_expr.clone(),
                    CoreExpr {
                        kind: CoreExprKind::LitInt(0),
                        ty: MonoType::Int,
                        span: iter_span,
                    },
                    idx_expr.clone(),
                ],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let loop_done_local = self.local_allocator.alloc();
        let run_loop_then_slice = CoreExpr {
            kind: CoreExprKind::Let {
                local: loop_done_local,
                value: Box::new(loop_expr),
                body: Box::new(slice_after_loop),
            },
            ty: result_ty.clone(),
            span,
        };

        let make_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_MAKE),
                    ty: MonoType::Function {
                        params: vec![MonoType::Int, MonoType::Void],
                        ret: Box::new(result_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![
                    CoreExpr {
                        kind: CoreExprKind::Local(len_local),
                        ty: MonoType::Int,
                        span: iter_span,
                    },
                    CoreExpr {
                        kind: CoreExprKind::LitVoid,
                        ty: MonoType::Void,
                        span: iter_span,
                    },
                ],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };

        // Wrap:
        // Let(r, iter,
        //   Let(cur, start,
        //     Let(end, end,
        //       Let(step, step,
        //         Let(len, computed_len,
        //           Let(idx, 0,
        //             Let(acc, Vector.make(len, ()),
        //               Loop ... )))))))
        Some(CoreExprKind::Let {
            local: r_tmp,
            value: Box::new(iter_expr),
            body: Box::new(CoreExpr {
                kind: CoreExprKind::Let {
                    local: cur_tmp,
                    value: Box::new(start_get),
                    body: Box::new(CoreExpr {
                        kind: CoreExprKind::Let {
                            local: end_tmp,
                            value: Box::new(end_get),
                            body: Box::new(CoreExpr {
                                kind: CoreExprKind::Let {
                                    local: step_tmp,
                                    value: Box::new(step_get),
                                    body: Box::new(CoreExpr {
                                        kind: CoreExprKind::Let {
                                            local: len_local,
                                            value: Box::new(len_expr),
                                            body: Box::new(CoreExpr {
                                                kind: CoreExprKind::Let {
                                                    local: idx_local,
                                                    value: Box::new(CoreExpr {
                                                        kind: CoreExprKind::LitInt(0),
                                                        ty: MonoType::Int,
                                                        span: iter_span,
                                                    }),
                                                    body: Box::new(CoreExpr {
                                                        kind: CoreExprKind::Let {
                                                            local: iter_idx_local,
                                                            value: Box::new(CoreExpr {
                                                                kind: CoreExprKind::LitInt(0),
                                                                ty: MonoType::Int,
                                                                span: iter_span,
                                                            }),
                                                            body: Box::new(CoreExpr {
                                                                kind: CoreExprKind::Let {
                                                                    local: acc_local,
                                                                    value: Box::new(make_call),
                                                                    body: Box::new(
                                                                        run_loop_then_slice,
                                                                    ),
                                                                },
                                                                ty: result_ty.clone(),
                                                                span,
                                                            }),
                                                        },
                                                        ty: result_ty.clone(),
                                                        span,
                                                    }),
                                                },
                                                ty: result_ty.clone(),
                                                span,
                                            }),
                                        },
                                        ty: result_ty.clone(),
                                        span,
                                    }),
                                },
                                ty: result_ty.clone(),
                                span,
                            }),
                        },
                        ty: result_ty.clone(),
                        span,
                    }),
                },
                ty: result_ty.clone(),
                span,
            }),
        })
    }

    /// Lower `collect x in iter { body }` where iter: Iterator<T>.
    ///
    /// Uses three vector-builder intrinsics (VECTOR_BUILDER_NEW/PUSH/FREEZE) to
    /// accumulate elements without requiring a mutable acc local:
    ///
    ///   Let(builder, VECTOR_BUILDER_NEW(),
    ///     Let(loop_it, iter_expr,
    ///       Loop {
    ///         Let(opt, ITERATOR_NEXT(loop_it),
    ///           Match(opt) {
    ///             None   → Break(VECTOR_BUILDER_FREEZE(builder))
    ///             Some(item) →
    ///               Let(x, item.value,
    ///                 Let(_, Assign(loop_it, item.rest),
    ///                   Let(elem, body,
    ///                     Let(_, VECTOR_BUILDER_PUSH(builder, elem),
    ///                       Continue))))
    ///           })
    ///       }))
    fn lower_iterator_collect(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let elem_ty = match &iter_expr.ty {
            MonoType::Named { type_id, args } if *type_id == ITERATOR_TYPE_ID => {
                args.first().cloned().unwrap_or(MonoType::Void)
            }
            _ => MonoType::Void,
        };
        let iter_ty = iter_expr.ty.clone();
        let item_ty = MonoType::Named {
            type_id: ITER_ITEM_TYPE_ID,
            args: vec![elem_ty.clone()],
        };
        let option_item_ty = MonoType::Named {
            type_id: OPTION_TYPE_ID,
            args: vec![item_ty.clone()],
        };

        // builder: Cell<Array<T>> — interior-mutable accumulator
        let builder_local = self.local_allocator.alloc_and_bind("__builder".to_string());
        let loop_it = self.local_allocator.alloc_and_bind("__it".to_string());
        let idx_counter =
            index_pattern.map(|_| self.local_allocator.alloc_and_bind("__idx".to_string()));

        // Loop scope
        self.local_allocator.push_scope();
        let opt_local = self.local_allocator.alloc_and_bind("__opt".to_string());
        let item_local = self.local_allocator.alloc_and_bind("__item".to_string());

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };
        let idx_user = index_pattern.and_then(|ip| {
            if let Pattern::Ident(name, _) = ip {
                Some(self.local_allocator.alloc_and_bind(name.clone()))
            } else {
                None
            }
        });
        let body_val_local = self.local_allocator.alloc_and_bind("__c_val".to_string());

        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();
        self.local_allocator.pop_scope();

        // VECTOR_BUILDER_PUSH(builder, body_val_local)
        let push_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_PUSH),
                    ty: MonoType::Void,
                    span: iter_span,
                }),
                args: vec![
                    CoreExpr {
                        kind: CoreExprKind::Local(builder_local),
                        ty: MonoType::Void,
                        span: iter_span,
                    },
                    CoreExpr {
                        kind: CoreExprKind::Local(body_val_local),
                        ty: body_ty,
                        span: iter_span,
                    },
                ],
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let continue_expr = CoreExpr {
            kind: CoreExprKind::Continue,
            ty: MonoType::Void,
            span: iter_span,
        };

        // Build tail: push → (optional idx increment) → continue
        let after_push = if let Some(idx_ctr) = idx_counter {
            let idx_inc = CoreExpr {
                kind: CoreExprKind::Assign {
                    local: idx_ctr,
                    value: Box::new(CoreExpr {
                        kind: CoreExprKind::BinOp {
                            op: BinOp::Add,
                            left: Box::new(CoreExpr {
                                kind: CoreExprKind::Local(idx_ctr),
                                ty: MonoType::Int,
                                span: iter_span,
                            }),
                            right: Box::new(CoreExpr {
                                kind: CoreExprKind::LitInt(1),
                                ty: MonoType::Int,
                                span: iter_span,
                            }),
                        },
                        ty: MonoType::Int,
                        span: iter_span,
                    }),
                },
                ty: MonoType::Void,
                span: iter_span,
            };
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: self.local_allocator.alloc(),
                    value: Box::new(idx_inc),
                    body: Box::new(continue_expr),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            continue_expr
        };
        let push_then_cont = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(push_call),
                body: Box::new(after_push),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        // Let(body_val, body_expr, push_then_cont)
        let with_body = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(push_then_cont),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Assign(loop_it, item.rest)
        let rest_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(item_local),
                    ty: item_ty.clone(),
                    span: iter_span,
                }),
                field: FieldId(1),
            },
            ty: iter_ty.clone(),
            span: iter_span,
        };
        let advance_it = CoreExpr {
            kind: CoreExprKind::Assign {
                local: loop_it,
                value: Box::new(rest_get),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let with_advance = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(advance_it),
                body: Box::new(with_body),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(x, item.value, with_advance)
        let value_get = CoreExpr {
            kind: CoreExprKind::RecordGet {
                target: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(item_local),
                    ty: item_ty,
                    span: iter_span,
                }),
                field: FieldId(0),
            },
            ty: elem_ty,
            span: iter_span,
        };
        // Optionally bind user index local
        let after_elem = if let Some(idx_u) = idx_user {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: idx_u,
                    value: Box::new(CoreExpr {
                        kind: CoreExprKind::Local(idx_counter.unwrap()),
                        ty: MonoType::Int,
                        span: iter_span,
                    }),
                    body: Box::new(with_advance),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            with_advance
        };
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(value_get),
                body: Box::new(after_elem),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // None arm: Break(VECTOR_BUILDER_FREEZE(builder))
        let freeze_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_FREEZE),
                    ty: MonoType::Function {
                        params: vec![MonoType::Void],
                        ret: Box::new(result_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![CoreExpr {
                    kind: CoreExprKind::Local(builder_local),
                    ty: MonoType::Void,
                    span: iter_span,
                }],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let break_freeze = CoreExpr {
            kind: CoreExprKind::Break {
                value: Some(Box::new(freeze_call)),
            },
            ty: result_ty.clone(),
            span: span,
        };

        // Match the option
        let match_expr = CoreExpr {
            kind: CoreExprKind::Match {
                scrutinee: Box::new(CoreExpr {
                    kind: CoreExprKind::Local(opt_local),
                    ty: MonoType::Named {
                        type_id: OPTION_TYPE_ID,
                        args: vec![MonoType::Void],
                    },
                    span: iter_span,
                }),
                arms: vec![
                    MatchArm {
                        pattern: CorePattern::Variant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(0),
                            fields: vec![],
                        },
                        body: break_freeze,
                    },
                    MatchArm {
                        pattern: CorePattern::Variant {
                            type_id: OPTION_TYPE_ID,
                            variant: VariantId(1),
                            fields: vec![CorePattern::Var(item_local)],
                        },
                        body: with_elem,
                    },
                ],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };

        // Let(opt, next_call, match_expr)
        let next_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::ITERATOR_NEXT),
                    ty: MonoType::Function {
                        params: vec![iter_ty.clone()],
                        ret: Box::new(option_item_ty),
                    },
                    span: iter_span,
                }),
                args: vec![CoreExpr {
                    kind: CoreExprKind::Local(loop_it),
                    ty: iter_ty.clone(),
                    span: iter_span,
                }],
            },
            ty: MonoType::Named {
                type_id: OPTION_TYPE_ID,
                args: vec![MonoType::Void],
            },
            span: iter_span,
        };
        let loop_body = CoreExpr {
            kind: CoreExprKind::Let {
                local: opt_local,
                value: Box::new(next_call),
                body: Box::new(match_expr),
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_body),
            },
            ty: result_ty.clone(),
            span,
        };

        // Optionally wrap with Let(idx_counter, 0, loop)
        let with_idx_init = if let Some(idx_ctr) = idx_counter {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: idx_ctr,
                    value: Box::new(CoreExpr {
                        kind: CoreExprKind::LitInt(0),
                        ty: MonoType::Int,
                        span,
                    }),
                    body: Box::new(loop_expr),
                },
                ty: result_ty.clone(),
                span,
            }
        } else {
            loop_expr
        };

        // Let(loop_it, iter_expr, with_idx_init)
        let with_it = CoreExpr {
            kind: CoreExprKind::Let {
                local: loop_it,
                value: Box::new(iter_expr),
                body: Box::new(with_idx_init),
            },
            ty: result_ty.clone(),
            span,
        };

        // builder_new_call
        let builder_new_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_NEW),
                    ty: MonoType::Void,
                    span: iter_span,
                }),
                args: vec![],
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(builder, builder_new_call, with_it)
        Some(CoreExprKind::Let {
            local: builder_local,
            value: Box::new(builder_new_call),
            body: Box::new(with_it),
        })
    }

    fn lower_dict_collect(
        &mut self,
        pattern: &Pattern,
        val_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let (key_ty, val_ty) = match &iter_expr.ty {
            MonoType::Dict(k, v) => (*k.clone(), *v.clone()),
            _ => return None,
        };

        let dict_tmp = self.local_allocator.alloc_and_bind("__cd_dict".to_string());
        let keys_tmp = self.local_allocator.alloc_and_bind("__cd_keys".to_string());
        let len_tmp = self.local_allocator.alloc_and_bind("__cd_len".to_string());
        let idx_tmp = self.local_allocator.alloc_and_bind("__cd_idx".to_string());
        let builder_local = self
            .local_allocator
            .alloc_and_bind("__cd_builder".to_string());

        let dict_local_expr = CoreExpr {
            kind: CoreExprKind::Local(dict_tmp),
            ty: iter_expr.ty.clone(),
            span: iter_span,
        };
        let keys_ty = MonoType::Vector(Box::new(key_ty.clone()));

        let keys_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::DICT_KEYS),
                    ty: MonoType::Function {
                        params: vec![iter_expr.ty.clone()],
                        ret: Box::new(keys_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![dict_local_expr.clone()],
            },
            ty: keys_ty.clone(),
            span: iter_span,
        };
        let keys_local_expr = CoreExpr {
            kind: CoreExprKind::Local(keys_tmp),
            ty: keys_ty.clone(),
            span: iter_span,
        };
        let len_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_LEN),
                    ty: MonoType::Function {
                        params: vec![keys_ty.clone()],
                        ret: Box::new(MonoType::Int),
                    },
                    span: iter_span,
                }),
                args: vec![keys_local_expr.clone()],
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_expr = CoreExpr {
            kind: CoreExprKind::Local(idx_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };
        let len_expr = CoreExpr {
            kind: CoreExprKind::Local(len_tmp),
            ty: MonoType::Int,
            span: iter_span,
        };

        self.local_allocator.push_scope();
        let key_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };
        let val_local = val_pattern.map(|vp| match vp {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        });
        let body_val_local = self.local_allocator.alloc_and_bind("__cd_val".to_string());
        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();
        self.local_allocator.pop_scope();

        let key_value = CoreExpr {
            kind: CoreExprKind::Index {
                base: Box::new(keys_local_expr),
                index: Box::new(idx_expr.clone()),
            },
            ty: key_ty.clone(),
            span: iter_span,
        };

        let idx_inc = CoreExpr {
            kind: CoreExprKind::Assign {
                local: idx_tmp,
                value: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Add,
                        left: Box::new(idx_expr.clone()),
                        right: Box::new(CoreExpr {
                            kind: CoreExprKind::LitInt(1),
                            ty: MonoType::Int,
                            span: iter_span,
                        }),
                    },
                    ty: MonoType::Int,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let push_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_PUSH),
                    ty: MonoType::Void,
                    span: iter_span,
                }),
                args: vec![
                    CoreExpr {
                        kind: CoreExprKind::Local(builder_local),
                        ty: MonoType::Void,
                        span: iter_span,
                    },
                    CoreExpr {
                        kind: CoreExprKind::Local(body_val_local),
                        ty: body_ty,
                        span: iter_span,
                    },
                ],
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let push_then_continue = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(push_call),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Continue,
                    ty: MonoType::Void,
                    span: iter_span,
                }),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let with_body_val = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(push_then_continue),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let with_val_binding = if let Some(vl) = val_local {
            let val_value = CoreExpr {
                kind: CoreExprKind::Call {
                    callee: Box::new(CoreExpr {
                        kind: CoreExprKind::GlobalFunc(prelude::DICT_GET_UNSAFE),
                        ty: MonoType::Function {
                            params: vec![iter_expr.ty.clone(), key_ty.clone()],
                            ret: Box::new(val_ty.clone()),
                        },
                        span: iter_span,
                    }),
                    args: vec![
                        dict_local_expr.clone(),
                        CoreExpr {
                            kind: CoreExprKind::Local(key_local),
                            ty: key_ty,
                            span: iter_span,
                        },
                    ],
                },
                ty: val_ty.clone(),
                span: iter_span,
            };
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: vl,
                    value: Box::new(val_value),
                    body: Box::new(with_body_val),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            with_body_val
        };
        let with_idx_inc = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(idx_inc),
                body: Box::new(with_val_binding),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let key_let = CoreExpr {
            kind: CoreExprKind::Let {
                local: key_local,
                value: Box::new(key_value),
                body: Box::new(with_idx_inc),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let loop_if = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(CoreExpr {
                    kind: CoreExprKind::BinOp {
                        op: BinOp::Ge,
                        left: Box::new(idx_expr),
                        right: Box::new(len_expr),
                    },
                    ty: MonoType::Bool,
                    span: iter_span,
                }),
                then_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span,
                }),
                else_branch: Box::new(key_let),
            },
            ty: MonoType::Void,
            span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_if),
            },
            ty: MonoType::Void,
            span,
        };
        let freeze_after_loop = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_FREEZE),
                    ty: MonoType::Function {
                        params: vec![MonoType::Void],
                        ret: Box::new(result_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![CoreExpr {
                    kind: CoreExprKind::Local(builder_local),
                    ty: MonoType::Void,
                    span: iter_span,
                }],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let run_loop_then_freeze = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(loop_expr),
                body: Box::new(freeze_after_loop),
            },
            ty: result_ty.clone(),
            span,
        };
        let builder_new_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_NEW),
                    ty: MonoType::Void,
                    span: iter_span,
                }),
                args: vec![],
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        Some(CoreExprKind::Let {
            local: dict_tmp,
            value: Box::new(iter_expr),
            body: Box::new(CoreExpr {
                kind: CoreExprKind::Let {
                    local: keys_tmp,
                    value: Box::new(keys_call),
                    body: Box::new(CoreExpr {
                        kind: CoreExprKind::Let {
                            local: len_tmp,
                            value: Box::new(len_call),
                            body: Box::new(CoreExpr {
                                kind: CoreExprKind::Let {
                                    local: idx_tmp,
                                    value: Box::new(CoreExpr {
                                        kind: CoreExprKind::LitInt(0),
                                        ty: MonoType::Int,
                                        span: iter_span,
                                    }),
                                    body: Box::new(CoreExpr {
                                        kind: CoreExprKind::Let {
                                            local: builder_local,
                                            value: Box::new(builder_new_call),
                                            body: Box::new(run_loop_then_freeze),
                                        },
                                        ty: result_ty.clone(),
                                        span,
                                    }),
                                },
                                ty: result_ty.clone(),
                                span,
                            }),
                        },
                        ty: result_ty.clone(),
                        span,
                    }),
                },
                ty: result_ty.clone(),
                span,
            }),
        })
    }

    fn lower_collect_while(
        &mut self,
        cond: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let loop_span = cond.span.merge(&body.span);
        let builder_local = self
            .local_allocator
            .alloc_and_bind("__cw_builder".to_string());

        self.local_allocator.push_scope();
        let body_val_local = self.local_allocator.alloc_and_bind("__cw_val".to_string());
        let cond_expr = self.lower_expr(cond)?;
        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();
        self.local_allocator.pop_scope();

        let push_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_PUSH),
                    ty: MonoType::Void,
                    span: loop_span,
                }),
                args: vec![
                    CoreExpr {
                        kind: CoreExprKind::Local(builder_local),
                        ty: MonoType::Void,
                        span: loop_span,
                    },
                    CoreExpr {
                        kind: CoreExprKind::Local(body_val_local),
                        ty: body_ty,
                        span: loop_span,
                    },
                ],
            },
            ty: MonoType::Void,
            span: loop_span,
        };
        let with_push_continue = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(push_call),
                body: Box::new(CoreExpr {
                    kind: CoreExprKind::Continue,
                    ty: MonoType::Void,
                    span: loop_span,
                }),
            },
            ty: MonoType::Void,
            span: loop_span,
        };
        // If body evaluates to Continue, push is skipped.
        let with_body_val = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(with_push_continue),
            },
            ty: MonoType::Void,
            span: loop_span,
        };

        let loop_if = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(cond_expr),
                then_branch: Box::new(with_body_val),
                else_branch: Box::new(CoreExpr {
                    kind: CoreExprKind::Break { value: None },
                    ty: MonoType::Void,
                    span: loop_span,
                }),
            },
            ty: MonoType::Void,
            span: loop_span,
        };
        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_if),
            },
            ty: MonoType::Void,
            span,
        };
        let freeze_after_loop = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_FREEZE),
                    ty: MonoType::Function {
                        params: vec![MonoType::Void],
                        ret: Box::new(result_ty.clone()),
                    },
                    span: loop_span,
                }),
                args: vec![CoreExpr {
                    kind: CoreExprKind::Local(builder_local),
                    ty: MonoType::Void,
                    span: loop_span,
                }],
            },
            ty: result_ty.clone(),
            span: loop_span,
        };
        let run_loop_then_freeze = CoreExpr {
            kind: CoreExprKind::Let {
                local: self.local_allocator.alloc(),
                value: Box::new(loop_expr),
                body: Box::new(freeze_after_loop),
            },
            ty: result_ty.clone(),
            span,
        };

        let builder_new_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_NEW),
                    ty: MonoType::Void,
                    span: loop_span,
                }),
                args: vec![],
            },
            ty: MonoType::Void,
            span: loop_span,
        };

        Some(CoreExprKind::Let {
            local: builder_local,
            value: Box::new(builder_new_call),
            body: Box::new(run_loop_then_freeze),
        })
    }

    fn lower_collect(
        &mut self,
        pattern: &Pattern,
        index_pattern: Option<&Pattern>,
        iter: &Expr,
        body: &Expr,
        result_ty: &MonoType,
        span: Span,
    ) -> Option<CoreExprKind> {
        let iter_ty = self.type_map.get_expr_type(iter.id).cloned();
        if matches!(iter_ty, Some(MonoType::Named { type_id, .. }) if type_id == RANGE_TYPE_ID) {
            return self.lower_range_collect(pattern, index_pattern, iter, body, result_ty, span);
        }
        if matches!(iter_ty, Some(MonoType::Named { type_id, .. }) if type_id == ITERATOR_TYPE_ID) {
            return self.lower_iterator_collect(
                pattern,
                index_pattern,
                iter,
                body,
                result_ty,
                span,
            );
        }
        if matches!(iter_ty, Some(MonoType::Dict(_, _))) {
            return self.lower_dict_collect(pattern, index_pattern, iter, body, result_ty, span);
        }

        // collect x in arr { body }
        // →
        // Let(arr_tmp, iter,
        //   Let(acc, [],
        //     Let(len, array_len(arr_tmp),
        //       Let(idx, 0,
        //         Loop {
        //           If(idx >= len, Break(acc),
        //             Let(x, arr_tmp[idx],
        //               Let(val, body,
        //                 Let(acc, append(acc, val),
        //                   Let(idx, idx+1, Continue)))))
        //         }))))

        let elem_ty = match iter_ty {
            Some(MonoType::Vector(inner)) => *inner,
            Some(MonoType::String) => MonoType::String,
            _ => MonoType::Void,
        };

        let iter_expr = self.lower_expr(iter)?;
        let iter_span = iter.span;

        let arr_tmp = self.local_allocator.alloc_and_bind("__c_arr".to_string());
        let builder_local = self
            .local_allocator
            .alloc_and_bind("__c_builder".to_string());
        let len_local = self.local_allocator.alloc_and_bind("__c_len".to_string());
        let idx_local = self.local_allocator.alloc_and_bind("__c_idx".to_string());

        let arr_local_expr = CoreExpr {
            kind: CoreExprKind::Local(arr_tmp),
            ty: iter_expr.ty.clone(),
            span: iter_span,
        };

        let len_func_id = match &iter_expr.ty {
            MonoType::String => prelude::STRING_LEN,
            _ => prelude::VECTOR_LEN,
        };

        // len = length(arr_tmp)
        let len_func = CoreExpr {
            kind: CoreExprKind::GlobalFunc(len_func_id),
            ty: MonoType::Function {
                params: vec![iter_expr.ty.clone()],
                ret: Box::new(MonoType::Int),
            },
            span: iter_span,
        };
        let len_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(len_func),
                args: vec![arr_local_expr.clone()],
            },
            ty: MonoType::Int,
            span: iter_span,
        };

        let idx_expr = CoreExpr {
            kind: CoreExprKind::Local(idx_local),
            ty: MonoType::Int,
            span: iter_span,
        };
        let len_expr = CoreExpr {
            kind: CoreExprKind::Local(len_local),
            ty: MonoType::Int,
            span: iter_span,
        };
        // Loop body: bind element, lower body, append, inc idx
        self.local_allocator.push_scope();

        let elem_local = match pattern {
            Pattern::Ident(name, _) => self.local_allocator.alloc_and_bind(name.clone()),
            _ => self.local_allocator.alloc(),
        };
        let idx_user = index_pattern.and_then(|ip| match ip {
            Pattern::Ident(name, _) => Some(self.local_allocator.alloc_and_bind(name.clone())),
            _ => None,
        });

        let elem_value =
            self.lower_index_core_expr(arr_local_expr, idx_expr.clone(), elem_ty, iter_span);

        let body_val_local = self.local_allocator.alloc_and_bind("__c_val".to_string());
        let body_expr = self.lower_expr(body)?;
        let body_ty = body_expr.ty.clone();

        self.local_allocator.pop_scope();

        // VECTOR_BUILDER_PUSH(builder, body_val_local)
        let push_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_PUSH),
                    ty: MonoType::Void,
                    span: iter_span,
                }),
                args: vec![
                    CoreExpr {
                        kind: CoreExprKind::Local(builder_local),
                        ty: MonoType::Void,
                        span: iter_span,
                    },
                    CoreExpr {
                        kind: CoreExprKind::Local(body_val_local),
                        ty: body_ty,
                        span: iter_span,
                    },
                ],
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // idx = idx + 1  [mutation via Assign]
        let one = CoreExpr {
            kind: CoreExprKind::LitInt(1),
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_plus_one = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Add,
                left: Box::new(idx_expr.clone()),
                right: Box::new(one),
            },
            ty: MonoType::Int,
            span: iter_span,
        };
        let idx_assign = CoreExpr {
            kind: CoreExprKind::Assign {
                local: idx_local,
                value: Box::new(idx_plus_one),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let continue_expr = CoreExpr {
            kind: CoreExprKind::Continue,
            ty: MonoType::Void,
            span: iter_span,
        };

        // Build innermost: VECTOR_BUILDER_PUSH(builder, val); Continue
        let tmp1 = self.local_allocator.alloc();
        let tail = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp1,
                value: Box::new(push_call),
                body: Box::new(continue_expr),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Let(val, body, tail)
        // If body produces Continue (e.g. `else { continue }`), the signal
        // propagates before acc is appended, skipping this element.
        let with_val = CoreExpr {
            kind: CoreExprKind::Let {
                local: body_val_local,
                value: Box::new(body_expr),
                body: Box::new(tail),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Assign(idx, idx+1); with_val
        // Increment BEFORE evaluating the body so that if body hits `continue`
        // the index is already advanced to the next element.
        let tmp2 = self.local_allocator.alloc();
        let with_idx = CoreExpr {
            kind: CoreExprKind::Let {
                local: tmp2,
                value: Box::new(idx_assign),
                body: Box::new(with_val),
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let with_idx_binding = if let Some(idx_local_user) = idx_user {
            CoreExpr {
                kind: CoreExprKind::Let {
                    local: idx_local_user,
                    value: Box::new(idx_expr.clone()),
                    body: Box::new(with_idx),
                },
                ty: MonoType::Void,
                span: iter_span,
            }
        } else {
            with_idx
        };

        // Let(elem, arr[idx], with_idx)
        let with_elem = CoreExpr {
            kind: CoreExprKind::Let {
                local: elem_local,
                value: Box::new(elem_value),
                body: Box::new(with_idx_binding),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        // Break condition: idx >= len → Break()
        // Freeze is performed once after the loop.
        let cond_ge = CoreExpr {
            kind: CoreExprKind::BinOp {
                op: BinOp::Ge,
                left: Box::new(idx_expr),
                right: Box::new(len_expr),
            },
            ty: MonoType::Bool,
            span: iter_span,
        };
        let break_acc = CoreExpr {
            kind: CoreExprKind::Break { value: None },
            ty: MonoType::Void,
            span: iter_span,
        };

        let loop_if = CoreExpr {
            kind: CoreExprKind::If {
                cond: Box::new(cond_ge),
                then_branch: Box::new(break_acc),
                else_branch: Box::new(with_elem),
            },
            ty: MonoType::Void,
            span: iter_span,
        };

        let loop_expr = CoreExpr {
            kind: CoreExprKind::Loop {
                body: Box::new(loop_if),
            },
            ty: MonoType::Void,
            span,
        };
        let freeze_after_loop = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_FREEZE),
                    ty: MonoType::Function {
                        params: vec![MonoType::Void],
                        ret: Box::new(result_ty.clone()),
                    },
                    span: iter_span,
                }),
                args: vec![CoreExpr {
                    kind: CoreExprKind::Local(builder_local),
                    ty: MonoType::Void,
                    span: iter_span,
                }],
            },
            ty: result_ty.clone(),
            span: iter_span,
        };
        let loop_done_local = self.local_allocator.alloc();
        let run_loop_then_freeze = CoreExpr {
            kind: CoreExprKind::Let {
                local: loop_done_local,
                value: Box::new(loop_expr),
                body: Box::new(freeze_after_loop),
            },
            ty: result_ty.clone(),
            span,
        };

        // Wrap:
        // Let(arr_tmp, iter,
        //   Let(builder, VECTOR_BUILDER_NEW(),
        //     Let(len, len_call, Let(idx, 0, loop))))
        let zero = CoreExpr {
            kind: CoreExprKind::LitInt(0),
            ty: MonoType::Int,
            span: iter_span,
        };

        let with_idx = CoreExpr {
            kind: CoreExprKind::Let {
                local: idx_local,
                value: Box::new(zero),
                body: Box::new(run_loop_then_freeze),
            },
            ty: result_ty.clone(),
            span,
        };
        let with_len = CoreExpr {
            kind: CoreExprKind::Let {
                local: len_local,
                value: Box::new(len_call),
                body: Box::new(with_idx),
            },
            ty: result_ty.clone(),
            span,
        };
        let builder_new_call = CoreExpr {
            kind: CoreExprKind::Call {
                callee: Box::new(CoreExpr {
                    kind: CoreExprKind::GlobalFunc(prelude::VECTOR_BUILDER_NEW),
                    ty: MonoType::Void,
                    span: iter_span,
                }),
                args: vec![],
            },
            ty: MonoType::Void,
            span: iter_span,
        };
        let with_builder_init = CoreExpr {
            kind: CoreExprKind::Let {
                local: builder_local,
                value: Box::new(builder_new_call),
                body: Box::new(with_len),
            },
            ty: result_ty.clone(),
            span,
        };

        Some(CoreExprKind::Let {
            local: arr_tmp,
            value: Box::new(iter_expr),
            body: Box::new(with_builder_init),
        })
    }

    // -----------------------------------------------------------------------
    // Lvalue chain desugaring
    // -----------------------------------------------------------------------

    /// Lower a lvalue assignment chain, returning (root_local, new_value_for_root).
    ///
    /// Recursion builds up the update from innermost to outermost:
    ///   `a.b.c = x`  → (a_local, RecordUpdate(Local(a), b_id, RecordUpdate(RecordGet(Local(a), b_id), c_id, x)))
    ///   `vec[i] = x` → (vec_local, Call(VECTOR_SET_UNSAFE, [Local(vec), lower(i), x]))
    ///   `m[k]   = x` → (m_local,   Call(DICT_SET,  [Local(m),   lower(k), x]))
    ///
    /// Returns None and records an error if the lvalue is invalid or unresolvable.
    fn lower_lvalue_chain(
        &mut self,
        lhs: &Expr,
        rhs_expr: CoreExpr,
    ) -> Option<(LocalId, CoreExpr)> {
        let span = lhs.span;
        match &lhs.kind {
            // Base case: simple identifier
            ExprKind::Ident(name) => {
                let local = match self.local_allocator.lookup(name) {
                    Some(l) => l,
                    None => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("unresolved lvalue name '{}' during lowering", name),
                            span,
                        });
                        return None;
                    }
                };
                Some((local, rhs_expr))
            }

            // Field write: base.field = rhs  →  RecordUpdate(lower(base), field_id, rhs)
            ExprKind::FieldAccess { base, field } => {
                let base_ty = self.type_map.get_expr_type(base.id).cloned();
                let type_id = match &base_ty {
                    Some(MonoType::Named { type_id, .. }) => *type_id,
                    _ => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("field write on non-record type {:?}", base_ty),
                            span,
                        });
                        return None;
                    }
                };
                let field_idx = match self.type_env.get_field_index(type_id, field) {
                    Some(i) => i,
                    None => {
                        self.errors.push(LowerError::UnknownField {
                            field: field.clone(),
                            type_name: format!("Type#{}", type_id.0),
                            span,
                        });
                        return None;
                    }
                };
                let base_core = self.lower_expr(base)?;
                let base_ty_clone = base_core.ty.clone();
                let update = CoreExpr {
                    kind: CoreExprKind::RecordUpdate {
                        base: Box::new(base_core),
                        field: FieldId(field_idx),
                        value: Box::new(rhs_expr),
                    },
                    ty: base_ty_clone,
                    span,
                };
                self.lower_lvalue_chain(base, update)
            }

            // Index write: base[index] = rhs  →  Call(VECTOR_SET_UNSAFE or DICT_SET, [lower(base), lower(index), rhs])
            ExprKind::Index { base, index } => {
                let base_ty = self.type_map.get_expr_type(base.id).cloned();
                let func_id = match &base_ty {
                    Some(MonoType::Vector(_)) => prelude::VECTOR_SET_UNSAFE,
                    Some(MonoType::Dict(_, _)) => prelude::DICT_SET,
                    _ => {
                        self.errors.push(LowerError::InternalError {
                            message: format!("index write on non-array/dict type {:?}", base_ty),
                            span,
                        });
                        return None;
                    }
                };
                let base_core = self.lower_expr(base)?;
                let idx_core = self.lower_expr(index)?;
                let base_ty_clone = base_core.ty.clone();
                let func_ty = MonoType::Function {
                    params: vec![
                        base_core.ty.clone(),
                        idx_core.ty.clone(),
                        rhs_expr.ty.clone(),
                    ],
                    ret: Box::new(base_ty_clone.clone()),
                };
                let func_expr = CoreExpr {
                    kind: CoreExprKind::GlobalFunc(func_id),
                    ty: func_ty,
                    span,
                };
                let update = CoreExpr {
                    kind: CoreExprKind::Call {
                        callee: Box::new(func_expr),
                        args: vec![base_core, idx_core, rhs_expr],
                    },
                    ty: base_ty_clone,
                    span,
                };
                self.lower_lvalue_chain(base, update)
            }

            _ => {
                self.errors.push(LowerError::InternalError {
                    message: "invalid lvalue in assignment".to_string(),
                    span,
                });
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// If `expr` is a simple assignment `name = value`, return `(name, value)`.
fn extract_simple_assign(expr: &Expr) -> Option<(&str, &Expr)> {
    if let ExprKind::Binary {
        op: BinOp::Assign,
        left,
        right,
    } = &expr.kind
    {
        if let ExprKind::Ident(name) = &left.kind {
            return Some((name.as_str(), right));
        }
    }
    None
}

fn stmt_span(stmt: &Stmt) -> Span {
    match stmt {
        Stmt::Let { span, .. } => *span,
        Stmt::For { span, .. } => *span,
        Stmt::ForCond { span, .. } => *span,
        Stmt::Expr(e) => e.span,
        Stmt::Break { span, .. } => *span,
        Stmt::Continue { span } => *span,
        Stmt::Return { span, .. } => *span,
        Stmt::Defer { span, .. } => *span,
    }
}

/// Lower `x == Type.Variant` / `x != Type.Variant` (unit variants only) into
/// a `match` so enum comparisons do not flow through primitive binop lowering.
fn lower_unit_variant_compare(
    op: BinOp,
    type_env: &TypeEnv,
    left: &CoreExpr,
    right: &CoreExpr,
    span: Span,
) -> Option<CoreExprKind> {
    if !matches!(op, BinOp::Eq | BinOp::Ne) {
        return None;
    }

    if let Some((type_id, variant)) = unit_variant_tag(&right) {
        if matches!(&left.ty, MonoType::Named { type_id: lhs_tid, .. } if *lhs_tid == type_id) {
            return Some(build_unit_variant_compare(
                op,
                left.clone(),
                type_id,
                variant,
                span,
            ));
        }
    }
    if let Some((type_id, variant)) = unit_variant_tag(&left) {
        if matches!(&right.ty, MonoType::Named { type_id: rhs_tid, .. } if *rhs_tid == type_id) {
            return Some(build_unit_variant_compare(
                op,
                right.clone(),
                type_id,
                variant,
                span,
            ));
        }
    }

    // Generic unit-enum equality/inequality: lower to a nested match so
    // named-type comparisons never reach primitive binop lowering.
    let (left_tid, right_tid) = match (&left.ty, &right.ty) {
        (
            MonoType::Named {
                type_id: left_tid, ..
            },
            MonoType::Named {
                type_id: right_tid, ..
            },
        ) if left_tid == right_tid => (*left_tid, *right_tid),
        _ => return None,
    };
    let _ = right_tid;
    let variant_ids = unit_sum_variant_ids(type_env, left_tid)?;
    Some(build_unit_sum_compare(
        op,
        left.clone(),
        right.clone(),
        left_tid,
        &variant_ids,
        span,
    ))
}

fn unit_sum_variant_ids(type_env: &TypeEnv, type_id: TypeId) -> Option<Vec<VariantId>> {
    let variants = type_env.get_variants(type_id)?;
    if variants.iter().any(|v| !v.fields.is_empty()) {
        return None;
    }
    Some((0..variants.len()).map(VariantId).collect())
}

fn build_unit_sum_compare(
    op: BinOp,
    left: CoreExpr,
    right: CoreExpr,
    type_id: TypeId,
    variants: &[VariantId],
    span: Span,
) -> CoreExprKind {
    let mut arms: Vec<MatchArm> = variants
        .iter()
        .map(|variant| MatchArm {
            pattern: CorePattern::Variant {
                type_id,
                variant: *variant,
                fields: vec![],
            },
            body: CoreExpr {
                kind: build_unit_variant_compare(op, right.clone(), type_id, *variant, span),
                ty: MonoType::Bool,
                span,
            },
        })
        .collect();

    let fallback = match op {
        BinOp::Eq => false,
        BinOp::Ne => true,
        _ => unreachable!("build_unit_sum_compare only handles Eq/Ne"),
    };
    arms.push(MatchArm {
        pattern: CorePattern::Wildcard,
        body: CoreExpr {
            kind: CoreExprKind::LitBool(fallback),
            ty: MonoType::Bool,
            span,
        },
    });

    CoreExprKind::Match {
        scrutinee: Box::new(left),
        arms,
    }
}

fn build_unit_variant_compare(
    op: BinOp,
    scrutinee: CoreExpr,
    type_id: TypeId,
    variant: VariantId,
    span: Span,
) -> CoreExprKind {
    let (on_match, on_other) = match op {
        BinOp::Eq => (true, false),
        BinOp::Ne => (false, true),
        _ => unreachable!("build_unit_variant_compare only handles Eq/Ne"),
    };
    CoreExprKind::Match {
        scrutinee: Box::new(scrutinee),
        arms: vec![
            MatchArm {
                pattern: CorePattern::Variant {
                    type_id,
                    variant,
                    fields: vec![],
                },
                body: CoreExpr {
                    kind: CoreExprKind::LitBool(on_match),
                    ty: MonoType::Bool,
                    span,
                },
            },
            MatchArm {
                pattern: CorePattern::Wildcard,
                body: CoreExpr {
                    kind: CoreExprKind::LitBool(on_other),
                    ty: MonoType::Bool,
                    span,
                },
            },
        ],
    }
}

fn unit_variant_tag(expr: &CoreExpr) -> Option<(TypeId, VariantId)> {
    match &expr.kind {
        CoreExprKind::Variant {
            type_id,
            variant,
            args,
        } if args.is_empty() => Some((*type_id, *variant)),
        _ => None,
    }
}

/// Walk a Core IR expression tree and collect all `Local(id)` references
/// where `id` is NOT in the `exclude` set. Deduplicates while preserving
/// first-occurrence order.
fn collect_local_refs(expr: &CoreExpr, exclude: &HashSet<LocalId>) -> Vec<LocalId> {
    let bound = exclude.clone();
    let mut captured = HashSet::new();
    let mut result = Vec::new();
    collect_local_refs_inner(expr, &bound, &mut captured, &mut result);
    result
}

fn collect_local_refs_inner(
    expr: &CoreExpr,
    bound: &HashSet<LocalId>,
    captured: &mut HashSet<LocalId>,
    result: &mut Vec<LocalId>,
) {
    use CoreExprKind::*;
    match &expr.kind {
        Local(id) => {
            if !bound.contains(id) && captured.insert(*id) {
                result.push(*id);
            }
        }
        Let { local, value, body } => {
            collect_local_refs_inner(value, bound, captured, result);
            let mut body_bound = bound.clone();
            body_bound.insert(*local);
            collect_local_refs_inner(body, &body_bound, captured, result);
        }
        Assign { local, value } => {
            if !bound.contains(local) && captured.insert(*local) {
                result.push(*local);
            }
            collect_local_refs_inner(value, bound, captured, result);
        }
        BinOp { left, right, .. } => {
            collect_local_refs_inner(left, bound, captured, result);
            collect_local_refs_inner(right, bound, captured, result);
        }
        UnOp { expr: inner, .. } => {
            collect_local_refs_inner(inner, bound, captured, result);
        }
        Call { callee, args } => {
            collect_local_refs_inner(callee, bound, captured, result);
            for a in args {
                collect_local_refs_inner(a, bound, captured, result);
            }
        }
        ContractCall { receiver, args, .. } => {
            collect_local_refs_inner(receiver, bound, captured, result);
            for a in args {
                collect_local_refs_inner(a, bound, captured, result);
            }
        }
        If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_local_refs_inner(cond, bound, captured, result);
            collect_local_refs_inner(then_branch, bound, captured, result);
            collect_local_refs_inner(else_branch, bound, captured, result);
        }
        Match { scrutinee, arms } => {
            collect_local_refs_inner(scrutinee, bound, captured, result);
            for arm in arms {
                let mut arm_bound = bound.clone();
                collect_pattern_bound_locals(&arm.pattern, &mut arm_bound);
                collect_local_refs_inner(&arm.body, &arm_bound, captured, result);
            }
        }
        Loop { body } => {
            collect_local_refs_inner(body, bound, captured, result);
        }
        Break { value: Some(v) } => {
            collect_local_refs_inner(v, bound, captured, result);
        }
        Return { value: Some(v) } => {
            collect_local_refs_inner(v, bound, captured, result);
        }
        Record { fields, .. } => {
            for (_, f) in fields {
                collect_local_refs_inner(f, bound, captured, result);
            }
        }
        RecordGet { target, .. } => {
            collect_local_refs_inner(target, bound, captured, result);
        }
        RecordUpdate { base, value, .. } => {
            collect_local_refs_inner(base, bound, captured, result);
            collect_local_refs_inner(value, bound, captured, result);
        }
        Variant { args, .. } => {
            for a in args {
                collect_local_refs_inner(a, bound, captured, result);
            }
        }
        ArrayLit { elements } => {
            for e in elements {
                collect_local_refs_inner(e, bound, captured, result);
            }
        }
        Index { base, index } => {
            collect_local_refs_inner(base, bound, captured, result);
            collect_local_refs_inner(index, bound, captured, result);
        }
        // MakeClosure's free_vars are LocalIds that must be visible to any wrapping lambda
        MakeClosure { free_vars, .. } => {
            for id in free_vars {
                if !bound.contains(id) && captured.insert(*id) {
                    result.push(*id);
                }
            }
        }
        Defer(inner) => {
            collect_local_refs_inner(inner, bound, captured, result);
        }
        // These don't contain Local refs (GlobalLocal is always reachable via globals)
        LitInt(_)
        | LitFloat(_)
        | LitBool(_)
        | LitStr(_)
        | LitVoid
        | GlobalFunc(_)
        | GlobalLocal(_)
        | Break { value: None }
        | Return { value: None }
        | Continue => {}
    }
}

fn collect_pattern_bound_locals(pattern: &CorePattern, bound: &mut HashSet<LocalId>) {
    use CorePattern::*;
    match pattern {
        Var(local_id) => {
            bound.insert(*local_id);
        }
        Variant { fields, .. } => {
            for field in fields {
                collect_pattern_bound_locals(field, bound);
            }
        }
        Wildcard | LitInt(_) | LitBool(_) | LitStr(_) => {}
    }
}

/// Extract a dotted name from an Ident/FieldAccess expression chain.
fn expr_as_dotted_name(expr: &Expr) -> Option<String> {
    match &expr.kind {
        ExprKind::Ident(name) => Some(name.clone()),
        ExprKind::FieldAccess { base, field } => {
            expr_as_dotted_name(base).map(|prefix| format!("{}.{}", prefix, field))
        }
        _ => None,
    }
}
