use std::collections::HashMap;

use super::ty::{FunctionSignature, MonoType, RecordField, TypeDef, TypeId, Variant};
use super::error::TypeError;
use crate::syntax::ast::Type as AstType;

/// Type environment - tracks user-defined type declarations
#[derive(Debug, Clone)]
pub struct TypeEnv {
    types: Vec<TypeDef>,
    type_names: HashMap<String, TypeId>,
    // For records: map (TypeId, field_name) -> field index
    record_fields: HashMap<(TypeId, String), usize>,
    // For sum types: map (TypeId, variant_name) -> variant index
    sum_variants: HashMap<(TypeId, String), usize>,
    // For inherent methods: map (TypeId, method_name) -> function name
    // Methods are functions whose first parameter is the receiver type
    methods: HashMap<(TypeId, String), String>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            type_names: HashMap::new(),
            record_fields: HashMap::new(),
            sum_variants: HashMap::new(),
            methods: HashMap::new(),
        }

        // Note: Built-in types (Int, Float, Bool, String, Void, Array) are handled
        // specially in resolve_type - they don't need TypeDef entries
    }

    /// Add a type definition and return its TypeId
    pub fn add_type(&mut self, def: TypeDef) -> TypeId {
        let type_id = TypeId(self.types.len() as u32);
        let name = def.name().to_string();

        // Build indices for field/variant lookup
        match &def {
            TypeDef::Record { fields, .. } => {
                for (i, field) in fields.iter().enumerate() {
                    self.record_fields
                        .insert((type_id, field.name.clone()), i);
                }
            }
            TypeDef::Sum { variants, .. } => {
                for (i, variant) in variants.iter().enumerate() {
                    self.sum_variants
                        .insert((type_id, variant.name.clone()), i);
                }
            }
            TypeDef::Alias { .. } => {}
        }

        self.types.push(def);
        self.type_names.insert(name, type_id);
        type_id
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
        match &def {
            TypeDef::Record { fields, .. } => {
                for (i, field) in fields.iter().enumerate() {
                    self.record_fields
                        .insert((type_id, field.name.clone()), i);
                }
            }
            TypeDef::Sum { variants, .. } => {
                for (i, variant) in variants.iter().enumerate() {
                    self.sum_variants
                        .insert((type_id, variant.name.clone()), i);
                }
            }
            TypeDef::Alias { .. } => {}
        }

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
                                note: "Int is a primitive type and takes no type arguments".to_string(),
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
                                note: "Float is a primitive type and takes no type arguments".to_string(),
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
                                note: "Bool is a primitive type and takes no type arguments".to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Bool)
                    }
                    "String" => {
                        if !args.is_empty() {
                            errors.push(TypeError::GenericNotSupported {
                                name: "String".to_string(),
                                span: *span,
                                note: "String is a primitive type and takes no type arguments".to_string(),
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
                                note: "Void is a primitive type and takes no type arguments".to_string(),
                            });
                            return Err(());
                        }
                        Ok(MonoType::Void)
                    }
                    "Array" => {
                        // Array<T> requires exactly one type argument
                        if args.len() != 1 {
                            errors.push(TypeError::UndefinedType {
                                name: if args.is_empty() {
                                    "Array (missing type argument)".to_string()
                                } else {
                                    format!("Array<...> (expected 1 type argument, found {})", args.len())
                                },
                                span: *span,
                            });
                            return Err(());
                        }
                        let elem_ty = self.resolve_type(&args[0], errors)?;
                        Ok(MonoType::Array(Box::new(elem_ty)))
                    }
                    "Dict" => {
                        // Dict<K, V> requires exactly two type arguments
                        if args.len() != 2 {
                            errors.push(TypeError::UndefinedType {
                                name: if args.is_empty() {
                                    "Dict (missing type arguments)".to_string()
                                } else {
                                    format!("Dict<...> (expected 2 type arguments, found {})", args.len())
                                },
                                span: *span,
                            });
                            return Err(());
                        }
                        let k_ty = self.resolve_type(&args[0], errors)?;
                        let v_ty = self.resolve_type(&args[1], errors)?;
                        Ok(MonoType::Dict(Box::new(k_ty), Box::new(v_ty)))
                    }
                    _ => {
                        // User-defined type
                        if !args.is_empty() {
                            // Type arguments not supported in Stage 2
                            errors.push(TypeError::GenericNotSupported {
                                name: name.clone(),
                                span: *span,
                                note: "Type arguments will be supported in Stage 5".to_string(),
                            });
                            return Err(());
                        }

                        // Look up in type environment
                        match self.lookup_type(name) {
                            Some(type_id) => {
                                // Expand aliases transparently: aliases are not nominal types
                                if let Some(TypeDef::Alias { target, .. }) = self.get_def(type_id) {
                                    Ok(target.clone())
                                } else {
                                    Ok(MonoType::named(type_id))
                                }
                            }
                            None => {
                                errors.push(TypeError::UndefinedType {
                                    name: name.clone(),
                                    span: *span,
                                });
                                Err(())
                            }
                        }
                    }
                }
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

    /// Register a method for a type
    /// Methods are functions whose first parameter is the receiver type
    pub fn add_method(&mut self, type_id: TypeId, method_name: String, func_name: String) {
        self.methods.insert((type_id, method_name), func_name);
    }

    /// Check if a type has a method with the given name
    pub fn has_method(&self, type_id: TypeId, method_name: &str) -> bool {
        self.methods.contains_key(&(type_id, method_name.to_string()))
    }

    /// Get the function name for a method
    /// Returns None if the method doesn't exist
    pub fn get_method_function(&self, type_id: TypeId, method_name: &str) -> Option<&String> {
        self.methods.get(&(type_id, method_name.to_string()))
    }

    /// Check if a type has a field with the given name (for collision detection)
    pub fn has_field(&self, type_id: TypeId, field_name: &str) -> bool {
        self.record_fields.contains_key(&(type_id, field_name.to_string()))
    }

    /// Number of registered types (for iterating all TypeIds)
    pub fn type_count(&self) -> usize {
        self.types.len()
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
}

impl ValueEnv {
    pub fn new() -> Self {
        let mut env = Self {
            functions: HashMap::new(),
            values: HashMap::new(),
            builtins: HashMap::new(),
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
                ret: Box::new(MonoType::Void), // Actually never returns, but Void for now
            },
        );

        // Type conversion builtins
        env.builtins.insert(
            "int_to_string".to_string(),
            MonoType::Function {
                params: vec![MonoType::Int],
                ret: Box::new(MonoType::String),
            },
        );
        env.builtins.insert(
            "float_to_string".to_string(),
            MonoType::Function {
                params: vec![MonoType::Float],
                ret: Box::new(MonoType::String),
            },
        );
        env.builtins.insert(
            "bool_to_string".to_string(),
            MonoType::Function {
                params: vec![MonoType::Bool],
                ret: Box::new(MonoType::String),
            },
        );
        env.builtins.insert(
            "string_to_string".to_string(),
            MonoType::Function {
                params: vec![MonoType::String],
                ret: Box::new(MonoType::String),
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

    /// Look up a value (checks functions, then values, then builtins)
    /// Returns a cloned MonoType to avoid lifetime issues
    pub fn lookup(&self, name: &str) -> Option<MonoType> {
        // Check functions first
        if let Some(sig) = self.functions.get(name) {
            // Only return function type if return type is known
            if let Some(ret) = &sig.ret {
                return Some(MonoType::Function {
                    params: sig.params.clone(),
                    ret: Box::new(ret.clone()),
                });
            } else {
                // Return type not yet inferred - this shouldn't happen in practice
                // since we update signatures after inference
                return None;
            }
        }

        // Then values
        if let Some(ty) = self.values.get(name) {
            return Some(ty.clone());
        }

        // Then builtins
        self.builtins.get(name).cloned()
    }

    /// Update a function signature (used after inferring return type)
    pub fn update_function(&mut self, sig: FunctionSignature) {
        self.functions.insert(sig.name.clone(), sig);
    }

    /// Get a function signature if it exists
    pub fn get_function(&self, name: &str) -> Option<&FunctionSignature> {
        self.functions.get(name)
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
