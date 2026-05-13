use std::collections::{HashMap, HashSet};

use super::error::TypeError;
use super::ty::{
    BUILTIN_BOOL_TYPE_ID, BUILTIN_BYTE_TYPE_ID, BUILTIN_DICT_TYPE_ID, BUILTIN_FLOAT_TYPE_ID,
    BUILTIN_INT_TYPE_ID, BUILTIN_STRING_TYPE_ID, BUILTIN_VECTOR_TYPE_ID, CELL_TYPE_ID,
    FunctionSignature, ITER_ITEM_TYPE_ID, ITERATOR_TYPE_ID, MonoType, OPTION_TYPE_ID,
    ORDER_TYPE_ID, RANGE_TYPE_ID, RESULT_TYPE_ID, RecordField, TASK_TYPE_ID, TypeDef, TypeId,
    UNFOLD_STEP_TYPE_ID, Variant,
};
use crate::intrinsics::signatures;
use crate::syntax::ast::Type as AstType;

/// Stored information for an inherent method, allowing method resolution
/// without requiring the defining module to be in the value environment.
#[derive(Debug, Clone)]
pub struct MethodInfo {
    /// Qualified function name used by the lowerer to resolve FuncId
    pub func_name: String,
    /// Full function signature for type checking. None for builtin methods
    /// registered early (before intrinsic signatures are available) — these
    /// fall back to ValueEnv lookup.
    pub signature: Option<FunctionSignature>,
}

/// Type environment - tracks user-defined type declarations
#[derive(Debug, Clone)]
pub struct TypeEnv {
    types: Vec<TypeDef>,
    type_names: HashMap<String, TypeId>,
    // For records: map (TypeId, field_name) -> field index
    record_fields: HashMap<(TypeId, String), usize>,
    // For sum types: map (TypeId, variant_name) -> variant index
    sum_variants: HashMap<(TypeId, String), usize>,
    // For inherent methods: map (TypeId, method_name) -> MethodInfo
    // Methods are functions whose first parameter is the receiver type
    methods: HashMap<(TypeId, String), MethodInfo>,
}

#[derive(Debug, Clone)]
pub struct TypeEnvBindingSnapshot {
    type_names: HashMap<String, TypeId>,
    methods: HashMap<(TypeId, String), MethodInfo>,
}

impl TypeEnv {
    pub fn new() -> Self {
        let mut env = Self {
            types: Vec::new(),
            type_names: HashMap::new(),
            record_fields: HashMap::new(),
            sum_variants: HashMap::new(),
            methods: HashMap::new(),
        };

        // Pre-register built-in parametric types with fixed TypeIds.
        // These MUST be registered first so they always receive their expected IDs.
        //
        // TypeId(0) = Option<T>  — sum type with None and Some(T)
        // TypeId(1) = Result<T,E> — sum type with Ok(T) and Err(E)
        // TypeId(2) = Cell<T>   — opaque mutable container
        //
        // The variant field types below are placeholders; the type checker uses
        // the args from MonoType::Named{type_id, args} to determine the actual
        // payload types at each use site.
        assert_eq!(
            env.add_type(TypeDef::Sum {
                name: "Option".to_string(),
                type_params: vec![],
                variants: vec![
                    Variant {
                        name: "None".to_string(),
                        fields: vec![]
                    },
                    Variant {
                        name: "Some".to_string(),
                        fields: vec![MonoType::Void]
                    },
                ],
                doc: Some("Optional value: None or Some(T).".to_string()),
            }),
            OPTION_TYPE_ID,
        );
        assert_eq!(
            env.add_type(TypeDef::Sum {
                name: "Result".to_string(),
                type_params: vec![],
                variants: vec![
                    Variant {
                        name: "Ok".to_string(),
                        fields: vec![MonoType::Void]
                    },
                    Variant {
                        name: "Err".to_string(),
                        fields: vec![MonoType::Void]
                    },
                ],
                doc: Some("Result value: Ok(T) or Err(E).".to_string()),
            }),
            RESULT_TYPE_ID,
        );
        assert_eq!(
            env.add_type(TypeDef::Record {
                name: "Cell".to_string(),
                type_params: vec![],
                fields: vec![],
                doc: Some("Mutable reference cell for imperative state.".to_string()),
            }),
            CELL_TYPE_ID,
        );
        assert_eq!(
            env.add_type(TypeDef::Record {
                name: "Range".to_string(),
                type_params: vec![],
                fields: vec![
                    RecordField {
                        name: "start".to_string(),
                        ty: MonoType::Int
                    },
                    RecordField {
                        name: "end".to_string(),
                        ty: MonoType::Int
                    },
                    RecordField {
                        name: "step".to_string(),
                        ty: MonoType::Int
                    },
                ],
                doc: Some("Range record { start, end, step } used by for loops.".to_string()),
            }),
            RANGE_TYPE_ID,
        );
        // TypeId(4) = Iterator<T> — opaque iterator, no fields (state is held at runtime)
        assert_eq!(
            env.add_type(TypeDef::Record {
                name: "Iterator".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![],
                doc: Some("Lazy iterator type.".to_string()),
            }),
            ITERATOR_TYPE_ID,
        );
        // TypeId(5) = IterItem<T> — record returned by Iterator.next: { value: T, rest: Iterator<T> }
        assert_eq!(
            env.add_type(TypeDef::Record {
                name: "IterItem".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![
                    RecordField {
                        name: "value".to_string(),
                        ty: MonoType::Var("T".to_string()),
                    },
                    RecordField {
                        name: "rest".to_string(),
                        ty: MonoType::Named {
                            type_id: ITERATOR_TYPE_ID,
                            args: vec![MonoType::Var("T".to_string())],
                        },
                    },
                ],
                doc: Some("Item returned by Iterator.next: value and rest iterator.".to_string()),
            }),
            ITER_ITEM_TYPE_ID,
        );
        // TypeId(6) = UnfoldStep<T,S> — sum type returned by step function
        //   Done | Yield(T, S)
        assert_eq!(
            env.add_type(TypeDef::Sum {
                name: "UnfoldStep".to_string(),
                type_params: vec!["T".to_string(), "S".to_string()],
                variants: vec![
                    Variant {
                        name: "Done".to_string(),
                        fields: vec![]
                    },
                    Variant {
                        name: "Yield".to_string(),
                        fields: vec![
                            MonoType::Var("T".to_string()),
                            MonoType::Var("S".to_string())
                        ],
                    },
                ],
                doc: Some(
                    "Step result for Iterator.unfold: Done or Yield(value, state).".to_string()
                ),
            }),
            UNFOLD_STEP_TYPE_ID,
        );

        // TypeId(7) = Order — comparison result: Lt, Eq, Gt
        assert_eq!(
            env.add_type(TypeDef::Sum {
                name: "Order".to_string(),
                type_params: vec![],
                variants: vec![
                    Variant {
                        name: "Lt".to_string(),
                        fields: vec![],
                    },
                    Variant {
                        name: "Eq".to_string(),
                        fields: vec![],
                    },
                    Variant {
                        name: "Gt".to_string(),
                        fields: vec![],
                    },
                ],
                doc: Some("Comparison result: Lt, Eq, or Gt.".to_string()),
            }),
            ORDER_TYPE_ID,
        );

        // TypeId(8) = Task<T> — opaque concurrent task handle
        assert_eq!(
            env.add_type(TypeDef::Record {
                name: "Task".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![],
                doc: Some("Concurrent task handle.".to_string()),
            }),
            TASK_TYPE_ID,
        );

        // Register all builtin method mappings.
        // These map (synthetic_type_id, method_name) → qualified function name
        // so that the env-driven resolution path works for all builtin types.
        // Signature is None — builtins are resolved via ValueEnv fallback.
        let builtin_methods: &[(TypeId, &str, &str)] = &[
            // Vector
            (BUILTIN_VECTOR_TYPE_ID, "len", "Vector.len"),
            (BUILTIN_VECTOR_TYPE_ID, "append", "Vector.append"),
            (BUILTIN_VECTOR_TYPE_ID, "concat", "Vector.concat"),
            (BUILTIN_VECTOR_TYPE_ID, "slice", "Vector.slice"),
            (BUILTIN_VECTOR_TYPE_ID, "get", "Vector.get"),
            (BUILTIN_VECTOR_TYPE_ID, "set", "Vector.set"),
            // String
            (BUILTIN_STRING_TYPE_ID, "len", "String.len"),
            (BUILTIN_STRING_TYPE_ID, "concat", "String.concat"),
            (BUILTIN_STRING_TYPE_ID, "slice", "String.slice"),
            (BUILTIN_STRING_TYPE_ID, "get", "String.get"),
            (BUILTIN_STRING_TYPE_ID, "to_string", "String.to_string"),
            (
                BUILTIN_STRING_TYPE_ID,
                "char_code_at",
                "String.char_code_at",
            ),
            (BUILTIN_STRING_TYPE_ID, "utf8_bytes", "String.utf8_bytes"),
            // Dict
            (BUILTIN_DICT_TYPE_ID, "len", "Dict.len"),
            (BUILTIN_DICT_TYPE_ID, "has", "Dict.has"),
            (BUILTIN_DICT_TYPE_ID, "get", "Dict.get"),
            (BUILTIN_DICT_TYPE_ID, "keys", "Dict.keys"),
            (BUILTIN_DICT_TYPE_ID, "remove", "Dict.remove"),
            (BUILTIN_DICT_TYPE_ID, "set", "Dict.set"),
            // Cell
            (CELL_TYPE_ID, "get", "Cell.get"),
            (CELL_TYPE_ID, "set", "Cell.set"),
            (CELL_TYPE_ID, "update", "Cell.update"),
            // Iterator
            (ITERATOR_TYPE_ID, "next", "Iterator.next"),
            // Task
            (TASK_TYPE_ID, "await", "Task.await"),
            // Option
            (OPTION_TYPE_ID, "ok_or", "Option.ok_or"),
            (OPTION_TYPE_ID, "ok_or_else", "Option.ok_or_else"),
            // Primitives
            (BUILTIN_INT_TYPE_ID, "to_string", "Int.to_string"),
            (BUILTIN_FLOAT_TYPE_ID, "to_string", "Float.to_string"),
            (BUILTIN_FLOAT_TYPE_ID, "bits", "Float.bits"),
            (BUILTIN_BOOL_TYPE_ID, "to_string", "Bool.to_string"),
            // Byte
            (BUILTIN_BYTE_TYPE_ID, "to_int", "Byte.to_int"),
            (BUILTIN_BYTE_TYPE_ID, "to_string", "Byte.to_string"),
        ];
        for &(type_id, method, func) in builtin_methods {
            env.add_method(type_id, method.to_string(), func.to_string(), None);
        }

        env
    }

    /// Add a type definition and return its TypeId
    pub fn add_type(&mut self, def: TypeDef) -> TypeId {
        self.add_type_with_binding(def, true)
    }

    /// Add an internal support type without binding its name in the user namespace.
    pub fn add_hidden_type(&mut self, def: TypeDef) -> TypeId {
        self.add_type_with_binding(def, false)
    }

    fn add_type_with_binding(&mut self, def: TypeDef, bind_name: bool) -> TypeId {
        let type_id = TypeId(self.types.len() as u32);
        let name = def.name().to_string();

        self.index_type_def(type_id, &def);
        self.types.push(def);
        if bind_name {
            self.type_names.insert(name, type_id);
        }
        type_id
    }

    fn index_type_def(&mut self, type_id: TypeId, def: &TypeDef) {
        match def {
            TypeDef::Record { fields, .. } => {
                for (i, field) in fields.iter().enumerate() {
                    self.record_fields.insert((type_id, field.name.clone()), i);
                }
            }
            TypeDef::Sum { variants, .. } => {
                for (i, variant) in variants.iter().enumerate() {
                    self.sum_variants.insert((type_id, variant.name.clone()), i);
                }
            }
            TypeDef::Alias { .. } => {}
        }
    }

    /// Update an existing type definition (preserves TypeId)
    pub fn update_type(&mut self, type_id: TypeId, def: TypeDef) {
        let idx = type_id.0 as usize;
        if idx >= self.types.len() {
            panic!("Invalid TypeId: {:?}", type_id);
        }

        // Clear old indices for this type_id
        self.record_fields.retain(|(id, _), _| *id != type_id);
        self.sum_variants.retain(|(id, _), _| *id != type_id);

        // Build new indices
        self.index_type_def(type_id, &def);

        self.types[idx] = def;
    }

    /// Look up a type by name
    pub fn lookup_type(&self, name: &str) -> Option<TypeId> {
        self.type_names.get(name).copied()
    }

    /// Register an additional name alias for an existing type (e.g. "module.TypeName" -> TypeId)
    /// Used to register qualified type names for imported modules.
    pub fn register_type_alias(&mut self, qualified_name: String, type_id: TypeId) {
        self.type_names.insert(qualified_name, type_id);
    }

    /// Remove a bare (unqualified) type name from the lookup table.
    ///
    /// Used after a dependency module finishes compiling so that its bare type
    /// names do not leak into subsequent modules' resolution.  The TypeId and
    /// TypeDef remain intact; only the bare-name lookup entry is removed.
    /// Cross-module access must always go through qualified aliases
    /// ("module.TypeName") registered via `register_type_alias`.
    pub fn remove_bare_type_name(&mut self, name: &str) {
        self.type_names.remove(name);
    }

    /// Snapshot type-name and method bindings for scoped compilation.
    ///
    /// This intentionally does not snapshot `types`, `record_fields`, or
    /// `sum_variants` so newly-defined type metadata remains available.
    pub fn snapshot_bindings(&self) -> TypeEnvBindingSnapshot {
        TypeEnvBindingSnapshot {
            type_names: self.type_names.clone(),
            methods: self.methods.clone(),
        }
    }

    /// Restore type-name and method bindings from a prior snapshot,
    /// but preserve any NEW method entries for user-defined types that
    /// were added during dependency compilation (for transitive method
    /// resolution). Builtin type methods (Vector, String, etc.) are
    /// fully restored to maintain prelude isolation for stdlib modules.
    pub fn restore_bindings(&mut self, snapshot: TypeEnvBindingSnapshot) {
        use super::ty::builtin_method_alias;

        // Collect new methods for user-defined types only.
        let new_user_methods: Vec<_> = self
            .methods
            .iter()
            .filter(|(key, _)| {
                // Keep if: (1) not in snapshot, and (2) not a builtin type
                builtin_method_alias(key.0).is_none() && !snapshot.methods.contains_key(key)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        self.type_names = snapshot.type_names;
        self.methods = snapshot.methods;

        // Re-insert new user-defined type method entries
        for (key, info) in new_user_methods {
            self.methods.entry(key).or_insert(info);
        }
    }

    /// Get a type definition by ID
    pub fn get_def(&self, type_id: TypeId) -> Option<&TypeDef> {
        self.types.get(type_id.0 as usize)
    }

    /// Get a record field index by name
    pub fn get_field_index(&self, type_id: TypeId, field_name: &str) -> Option<usize> {
        self.record_fields
            .get(&(type_id, field_name.to_string()))
            .copied()
    }

    /// Get a sum type variant index by name
    pub fn get_variant_index(&self, type_id: TypeId, variant_name: &str) -> Option<usize> {
        self.sum_variants
            .get(&(type_id, variant_name.to_string()))
            .copied()
    }

    /// Get record fields for a type
    pub fn get_record_fields(&self, type_id: TypeId) -> Option<&[RecordField]> {
        match self.get_def(type_id)? {
            TypeDef::Record { fields, .. } => Some(fields),
            _ => None,
        }
    }

    /// Get sum type variants for a type
    pub fn get_variants(&self, type_id: TypeId) -> Option<&[Variant]> {
        match self.get_def(type_id)? {
            TypeDef::Sum { variants, .. } => Some(variants),
            _ => None,
        }
    }

    /// Resolve an AST type annotation to a MonoType
    /// Shared by both Resolver and TypeChecker to avoid duplication
    pub fn resolve_type(&self, ty: &AstType, errors: &mut Vec<TypeError>) -> Result<MonoType, ()> {
        match ty {
            AstType::Named { name, args, span } => {
                // Check for built-in types first
                match name.as_str() {
                    "Int" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "Int".to_string(),
                                span: *span,
                                note: "Int is a primitive type and takes no type arguments"
                                    .to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Int)
                    }
                    "Float" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "Float".to_string(),
                                span: *span,
                                note: "Float is a primitive type and takes no type arguments"
                                    .to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Float)
                    }
                    "Bool" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "Bool".to_string(),
                                span: *span,
                                note: "Bool is a primitive type and takes no type arguments"
                                    .to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Bool)
                    }
                    "Byte" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "Byte".to_string(),
                                span: *span,
                                note: "Byte is a primitive type and takes no type arguments"
                                    .to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Byte)
                    }
                    "String" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "String".to_string(),
                                span: *span,
                                note: "String is a primitive type and takes no type arguments"
                                    .to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::String)
                    }
                    "Void" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "Void".to_string(),
                                span: *span,
                                note: "Void is a primitive type and takes no type arguments"
                                    .to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Void)
                    }
                    "Vector" => {
                        // Vector<T> requires exactly one type argument
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: if args.is_empty() {
                                    "Vector (missing type argument)".to_string()
                                } else {
                                    format!(
                                        "Vector<...> (expected 1 type argument, found {})",
                                        args.len()
                                    )
                                },
                                span: *span,
                            });
                            return Err(());
                        }
                        let elem_ty = self.resolve_type(&args[0], errors)?;
                        Ok(MonoType::Vector(Box::new(elem_ty)))
                    }
                    "Dict" => {
                        // Dict<K, V> requires exactly two type arguments
                        if args.len() != 2 {
                            errors.push(TypeError::UndefinedType {
                                name: if args.is_empty() {
                                    "Dict (missing type arguments)".to_string()
                                } else {
                                    format!(
                                        "Dict<...> (expected 2 type arguments, found {})",
                                        args.len()
                                    )
                                },
                                span: *span,
                            });
                            return Err(());
                        }
                        let k_ty = self.resolve_type(&args[0], errors)?;
                        match &k_ty {
                            MonoType::Int | MonoType::String | MonoType::Byte => {}
                            _ => {
                                errors.push(TypeError::InvalidDictKey {
                                    key_type: k_ty.clone(),
                                    span: *span,
                                });
                                return Err(());
                            }
                        }
                        let v_ty = self.resolve_type(&args[1], errors)?;
                        Ok(MonoType::Dict(Box::new(k_ty), Box::new(v_ty)))
                    }
                    "Option" => {
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "Option (expected 1 type argument, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let inner = self.resolve_type(&args[0], errors)?;
                        return Ok(MonoType::Named {
                            type_id: OPTION_TYPE_ID,
                            args: vec![inner],
                        });
                    }
                    "Result" => {
                        if args.len() != 2 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "Result (expected 2 type arguments, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let t = self.resolve_type(&args[0], errors)?;
                        let e = self.resolve_type(&args[1], errors)?;
                        return Ok(MonoType::Named {
                            type_id: RESULT_TYPE_ID,
                            args: vec![t, e],
                        });
                    }
                    "Cell" => {
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "Cell (expected 1 type argument, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let inner = self.resolve_type(&args[0], errors)?;
                        return Ok(MonoType::Named {
                            type_id: CELL_TYPE_ID,
                            args: vec![inner],
                        });
                    }
                    "Iterator" => {
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "Iterator (expected 1 type argument, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let elem = self.resolve_type(&args[0], errors)?;
                        return Ok(MonoType::Named {
                            type_id: ITERATOR_TYPE_ID,
                            args: vec![elem],
                        });
                    }
                    "Task" => {
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "Task (expected 1 type argument, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let inner = self.resolve_type(&args[0], errors)?;
                        return Ok(MonoType::Named {
                            type_id: TASK_TYPE_ID,
                            args: vec![inner],
                        });
                    }
                    "IterItem" => {
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "IterItem (expected 1 type argument, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let elem = self.resolve_type(&args[0], errors)?;
                        return Ok(MonoType::Named {
                            type_id: ITER_ITEM_TYPE_ID,
                            args: vec![elem],
                        });
                    }
                    "UnfoldStep" => {
                        if args.len() != 2 {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "UnfoldStep (expected 2 type arguments, found {})",
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }
                        let t = self.resolve_type(&args[0], errors)?;
                        let s = self.resolve_type(&args[1], errors)?;
                        return Ok(MonoType::Named {
                            type_id: UNFOLD_STEP_TYPE_ID,
                            args: vec![t, s],
                        });
                    }
                    _ => {
                        // User-defined type — look up in type environment
                        let type_id = match self.lookup_type(name) {
                            Some(id) => id,
                            None => {
                                errors.push(TypeError::UndefinedType {
                                    name: name.clone(),
                                    span: *span,
                                });
                                return Err(());
                            }
                        };

                        // Aliases: expand transparently, but don't accept type args
                        if let Some(TypeDef::Alias { target, .. }) = self.get_def(type_id) {
                            if !args.is_empty() {
                                errors.push(TypeError::GenericNotSupported {
                                    name: name.clone(),
                                    span: *span,
                                    note: "Type aliases cannot take type arguments".to_string(),
                                });
                                return Err(());
                            }
                            return Ok(target.clone());
                        }

                        // Check arity against declared type_params
                        let expected_arity = self
                            .get_def(type_id)
                            .map(|d| d.type_params().len())
                            .unwrap_or(0);
                        if args.len() != expected_arity {
                            errors.push(TypeError::UndefinedType {
                                name: format!(
                                    "{} (expected {} type arg(s), found {})",
                                    name,
                                    expected_arity,
                                    args.len()
                                ),
                                span: *span,
                            });
                            return Err(());
                        }

                        let resolved_args: Vec<MonoType> = args
                            .iter()
                            .map(|a| self.resolve_type(a, errors))
                            .collect::<Result<_, _>>()?;
                        Ok(MonoType::Named {
                            type_id,
                            args: resolved_args,
                        })
                    }
                }
            }
            AstType::Record { span, .. } => {
                errors.push(TypeError::UnsupportedFeature {
                    feature: "anonymous record types in this type position",
                    span: *span,
                    note:
                        "Anonymous record types are only supported as a single enum variant payload"
                            .to_string(),
                });
                Err(())
            }
            AstType::Function { params, ret, .. } => {
                let mut resolved_params = Vec::new();
                for param in params {
                    let ty = self.resolve_type(param, errors)?;
                    resolved_params.push(ty);
                }
                let resolved_ret = Box::new(self.resolve_type(ret, errors)?);
                Ok(MonoType::Function {
                    params: resolved_params,
                    ret: resolved_ret,
                })
            }
        }
    }

    /// Register a method for a type.
    /// Methods are functions whose first parameter is the receiver type.
    /// Pass `signature` for user-defined methods; builtins registered early
    /// can pass `None` and rely on ValueEnv fallback.
    pub fn add_method(
        &mut self,
        type_id: TypeId,
        method_name: String,
        func_name: String,
        signature: Option<FunctionSignature>,
    ) {
        self.methods.insert(
            (type_id, method_name),
            MethodInfo {
                func_name,
                signature,
            },
        );
    }

    pub fn remove_method(&mut self, type_id: TypeId, method_name: &str) {
        self.methods.remove(&(type_id, method_name.to_string()));
    }

    /// Check if a type has a method with the given name
    pub fn has_method(&self, type_id: TypeId, method_name: &str) -> bool {
        self.methods
            .contains_key(&(type_id, method_name.to_string()))
    }

    /// Get the full method info (signature + func name) for a method.
    pub fn get_method(&self, type_id: TypeId, method_name: &str) -> Option<&MethodInfo> {
        self.methods.get(&(type_id, method_name.to_string()))
    }

    /// Get the function name for a method (convenience wrapper).
    pub fn get_method_function(&self, type_id: TypeId, method_name: &str) -> Option<&String> {
        self.methods
            .get(&(type_id, method_name.to_string()))
            .map(|info| &info.func_name)
    }

    /// Check if a type has a field with the given name (for collision detection)
    pub fn has_field(&self, type_id: TypeId, field_name: &str) -> bool {
        self.record_fields
            .contains_key(&(type_id, field_name.to_string()))
    }

    /// Number of registered types (for iterating all TypeIds)
    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    /// Iterate all methods registered for a given type.
    /// Returns (method_name, qualified_function_name) pairs.
    pub fn methods_for_type(&self, type_id: TypeId) -> Vec<(&str, &str)> {
        self.methods
            .iter()
            .filter_map(move |((tid, method_name), info)| {
                if *tid == type_id {
                    Some((method_name.as_str(), info.func_name.as_str()))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Iterate all type names and their TypeIds.
    pub fn all_type_names(&self) -> impl Iterator<Item = (&str, TypeId)> {
        self.type_names
            .iter()
            .map(|(name, id)| (name.as_str(), *id))
    }
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// Value environment - tracks functions and values
#[derive(Debug, Clone)]
pub struct ValueEnv {
    functions: HashMap<String, FunctionSignature>,
    values: HashMap<String, MonoType>,
    builtins: HashMap<String, MonoType>,
    /// Extern module namespace names (e.g., "console", "Math") derived from
    /// `extern "module" fn ...` declarations.  Used by the checker and lowerer
    /// to recognise `module.func(...)` as a qualified extern call.
    extern_namespaces: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct ValueEnvBindingSnapshot {
    functions: HashMap<String, FunctionSignature>,
    values: HashMap<String, MonoType>,
    extern_namespaces: HashSet<String>,
}

impl ValueEnv {
    pub fn new() -> Self {
        let mut env = Self::new_without_intrinsic_signatures();
        for sig in signatures::function_signatures() {
            env.add_function(sig);
        }
        env
    }

    pub(crate) fn new_without_intrinsic_signatures() -> Self {
        let mut env = Self {
            functions: HashMap::new(),
            values: HashMap::new(),
            builtins: HashMap::new(),
            extern_namespaces: HashSet::new(),
        };

        // Register built-in functions
        env.builtins.insert(
            "println".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Void),
            },
        );
        env.builtins.insert(
            "print".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Void),
            },
        );
        env.builtins.insert(
            "error".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Never),
            },
        );
        env.builtins.insert(
            "eprint".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Void),
            },
        );
        env.builtins.insert(
            "eprintln".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Void),
            },
        );

        env.builtins.insert(
            "string_len".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Int),
            },
        );
        env.builtins.insert(
            "string_concat".to_string(),
            MonoType::Function {
                params: vec![MonoType::String, MonoType::String],
                ret: Box::new(MonoType::String),
            },
        );

        env.builtins.insert(
            "range_from".to_string(),
            MonoType::Function {
                params: vec![MonoType::Int, MonoType::Int],
                ret: Box::new(MonoType::named(RANGE_TYPE_ID)),
            },
        );
        env.builtins.insert(
            "range".to_string(),
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::named(RANGE_TYPE_ID)),
            },
        );
        env.builtins.insert(
            "range_step".to_string(),
            MonoType::Function {
                params: vec![MonoType::Int, MonoType::Int, MonoType::Int],
                ret: Box::new(MonoType::named(RANGE_TYPE_ID)),
            },
        );

        env.builtins.insert(
            "__host_read_file".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Named {
                    type_id: RESULT_TYPE_ID,
                    args: vec![MonoType::Vector(Box::new(MonoType::Byte)), MonoType::String],
                }),
            },
        );
        env.builtins.insert(
            "__host_write_file".to_string(),
            MonoType::Function {
                params: vec![MonoType::String, MonoType::String],
                ret: Box::new(MonoType::Void),
            },
        );
        env.builtins.insert(
            "__host_write_bytes".to_string(),
            MonoType::Function {
                params: vec![MonoType::String, MonoType::Vector(Box::new(MonoType::Byte))],
                ret: Box::new(MonoType::Void),
            },
        );
        env.builtins.insert(
            "__host_mkdirp".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Void),
            },
        );
        env.builtins.insert(
            "__host_list_dir".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Vector(Box::new(MonoType::String))),
            },
        );
        env.builtins.insert(
            "__host_exists".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Bool),
            },
        );
        env.builtins.insert(
            "__host_args".to_string(),
            MonoType::Function {
                params: vec![],
                ret: Box::new(MonoType::Vector(Box::new(MonoType::String))),
            },
        );
        env.builtins.insert(
            "__host_env".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::Vector(Box::new(MonoType::String))),
            },
        );
        env.builtins.insert(
            "__host_cwd".to_string(),
            MonoType::Function {
                params: vec![],
                ret: Box::new(MonoType::String),
            },
        );
        env.builtins.insert(
            "__host_exit".to_string(),
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::Never),
            },
        );
        env.builtins.insert(
            "__host_now".to_string(),
            MonoType::Function {
                params: vec![],
                ret: Box::new(MonoType::Float),
            },
        );
        env.builtins.insert(
            "__host_run_wasm".to_string(),
            MonoType::Function {
                params: vec![
                    MonoType::Vector(Box::new(MonoType::Byte)),
                    MonoType::Vector(Box::new(MonoType::String)),
                ],
                ret: Box::new(MonoType::Int),
            },
        );
        env.builtins.insert(
            "__host_stdin_read_chunk".to_string(),
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::Vector(Box::new(MonoType::Byte))),
            },
        );
        env.builtins.insert(
            "__host_stdout_write_bytes".to_string(),
            MonoType::Function {
                params: vec![MonoType::Vector(Box::new(MonoType::Byte))],
                ret: Box::new(MonoType::Void),
            },
        );

        // Note: len() is intentionally NOT pre-registered as a builtin here.
        // It will be handled specially in check.rs::synth_call() to support both
        // String and Array<T> monomorphically (without requiring generics).
        // See the plan's "Built-in Special Cases" section.
        //
        // If len() is called before we implement the type checker, it will error
        // as "undefined variable" - this is expected and will be fixed when
        // check.rs is implemented.

        env
    }

    /// Add a function signature
    pub fn add_function(&mut self, sig: FunctionSignature) {
        self.functions.insert(sig.name.clone(), sig);
    }

    /// Add a top-level value binding
    pub fn add_value(&mut self, name: String, ty: MonoType) {
        self.values.insert(name, ty);
    }

    pub fn remove_function(&mut self, name: &str) {
        self.functions.remove(name);
    }

    pub fn remove_value(&mut self, name: &str) {
        self.values.remove(name);
    }

    /// Look up a value (checks functions, then values, then builtins)
    /// Returns a cloned MonoType to avoid lifetime issues
    pub fn lookup(&self, name: &str) -> Option<MonoType> {
        // Check functions first
        if let Some(sig) = self.functions.get(name) {
            // If return type is not yet inferred (no explicit annotation), default to Void.
            // This allows top-level expressions to call functions before their bodies are
            // type-checked in pass 2; the actual return type is verified when the body is checked.
            let ret = sig.ret.clone().unwrap_or(MonoType::Void);
            return Some(MonoType::Function {
                params: sig.params.clone(),
                ret: Box::new(ret),
            });
        }

        // Then values
        if let Some(ty) = self.values.get(name) {
            return Some(ty.clone());
        }

        // Then builtins
        self.builtins.get(name).cloned()
    }

    /// True when name resolves to an internal host builtin through normal lookup
    /// precedence (i.e. not shadowed by user function/value bindings).
    pub fn is_visible_internal_host_builtin(&self, name: &str) -> bool {
        name.starts_with("__host_")
            && !self.functions.contains_key(name)
            && !self.values.contains_key(name)
            && self.builtins.contains_key(name)
    }

    /// Apply meta-variable substitution to all top-level value bindings.
    /// Called when check_function clears meta_subst so that cross-item metas
    /// (e.g. from `x := Dict.new()`) are propagated before the substitution
    /// map is dropped.
    pub fn zonk_values(&mut self, meta_subst: &std::collections::HashMap<u32, MonoType>) {
        use crate::types::ty::zonk_ty;
        for ty in self.values.values_mut() {
            *ty = zonk_ty(ty, meta_subst);
        }
    }

    /// Update a function signature (used after inferring return type)
    pub fn update_function(&mut self, sig: FunctionSignature) {
        self.functions.insert(sig.name.clone(), sig);
    }

    /// Get a function signature if it exists
    pub fn get_function(&self, name: &str) -> Option<&FunctionSignature> {
        self.functions.get(name)
    }

    /// True when the name is bound by a top-level value (not a function/builtin).
    pub fn has_value_binding(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    /// Iterate all registered function signatures.
    pub fn all_functions(&self) -> impl Iterator<Item = (&str, &FunctionSignature)> {
        self.functions.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterate all top-level value bindings.
    pub fn all_values(&self) -> impl Iterator<Item = (&str, &MonoType)> {
        self.values.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Iterate all builtin functions.
    pub fn all_builtins(&self) -> impl Iterator<Item = (&str, &MonoType)> {
        self.builtins.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Snapshot function/value bindings for scoped compilation.
    ///
    /// Builtins are immutable and intentionally excluded.
    pub fn snapshot_bindings(&self) -> ValueEnvBindingSnapshot {
        ValueEnvBindingSnapshot {
            functions: self.functions.clone(),
            values: self.values.clone(),
            extern_namespaces: self.extern_namespaces.clone(),
        }
    }

    /// Restore function/value bindings from a prior snapshot.
    pub fn restore_bindings(&mut self, snapshot: ValueEnvBindingSnapshot) {
        self.functions = snapshot.functions;
        self.values = snapshot.values;
        self.extern_namespaces = snapshot.extern_namespaces;
    }

    /// Register an extern module namespace name (e.g., "console" from `extern "console" fn ...`).
    pub fn add_extern_namespace(&mut self, name: String) {
        self.extern_namespaces.insert(name);
    }

    /// Check whether a name is a registered extern namespace.
    pub fn is_extern_namespace(&self, name: &str) -> bool {
        self.extern_namespaces.contains(name)
    }

    /// Get all extern namespace names.
    pub fn extern_namespaces(&self) -> &HashSet<String> {
        &self.extern_namespaces
    }
}

impl Default for ValueEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// Local environment for function bodies - supports scoping and shadowing
#[derive(Debug, Clone)]
pub struct LocalEnv {
    scopes: Vec<HashMap<String, MonoType>>,
}

impl LocalEnv {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Push a new scope (for blocks, case arms, etc.)
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope
    /// If only the root scope remains, this is a no-op to avoid crashing the compiler
    pub fn pop_scope(&mut self) {
        // Guard against popping the root scope - noop instead of panic
        // to prevent type checker bugs from crashing the compiler
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
        // else: silently ignore - root scope cannot be popped
    }

    /// Bind a variable in the current scope
    pub fn bind(&mut self, name: String, ty: MonoType) {
        self.scopes.last_mut().unwrap().insert(name, ty);
    }

    /// Look up a variable (searches from innermost to outermost scope)
    pub fn lookup(&self, name: &str) -> Option<&MonoType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }
}

impl Default for LocalEnv {
    fn default() -> Self {
        Self::new()
    }
}
