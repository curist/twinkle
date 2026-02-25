use crate::syntax::ast::{
    FunctionDecl, Item, SourceFile, Type as AstType, TypeDecl,
    TypeDef as AstTypeDef,
};
use crate::syntax::span::Span;
use super::env::{TypeEnv, ValueEnv};
use super::error::TypeError;
use super::ty::{FunctionSignature, MonoType, RecordField, TypeDef, TypeId, Variant};
use std::collections::{HashMap, HashSet};
use std::mem;

/// Two-pass name resolver for type and function declarations
///
/// Pass 1: Collect all type and function names, detect duplicates, reject generics
/// Pass 2: Resolve all type references, build TypeEnv and ValueEnv
pub struct Resolver {
    type_env: TypeEnv,
    value_env: ValueEnv,
    errors: Vec<TypeError>,

    // Track type declarations for Pass 2
    type_decls: HashMap<String, TypeDecl>,
    type_spans: HashMap<String, Span>,

    // Track function declarations for Pass 2
    function_decls: HashMap<String, FunctionDecl>,
    function_spans: HashMap<String, Span>,
}

impl Resolver {
    /// Resolve all names in a source file
    /// Returns TypeEnv and ValueEnv on success, or a list of errors on failure
    pub fn resolve(source_file: &SourceFile) -> Result<(TypeEnv, ValueEnv), Vec<TypeError>> {
        let mut resolver = Resolver {
            type_env: TypeEnv::new(),
            value_env: ValueEnv::new(),
            errors: Vec::new(),
            type_decls: HashMap::new(),
            type_spans: HashMap::new(),
            function_decls: HashMap::new(),
            function_spans: HashMap::new(),
        };

        // Pass 1: Collect all declarations and check for duplicates/generics
        resolver.collect_declarations(source_file);

        // Early return if Pass 1 had errors
        if !resolver.errors.is_empty() {
            return Err(resolver.errors);
        }

        // Pass 2: Resolve type references and build environments
        resolver.resolve_type_references();
        resolver.resolve_function_signatures();

        // Check for circular type aliases
        resolver.detect_circular_aliases();

        if !resolver.errors.is_empty() {
            Err(resolver.errors)
        } else {
            Ok((resolver.type_env, resolver.value_env))
        }
    }

    /// Resolve all names using a shared CompilationContext (multi-module mode).
    ///
    /// Types/functions from imported modules are already registered in
    /// `ctx.type_env`/`ctx.value_env` via `CompilationContext::register_module_exports`.
    /// This function adds the current module's declarations to those shared envs
    /// and writes them back.
    pub fn resolve_with_context(
        source_file: &SourceFile,
        ctx: &mut crate::module::context::CompilationContext,
    ) -> Result<(), Vec<TypeError>> {
        // Move shared envs out of ctx so the resolver can own them temporarily
        let type_env = mem::replace(&mut ctx.type_env, TypeEnv::new());
        let value_env = mem::replace(&mut ctx.value_env, ValueEnv::new());

        let mut resolver = Resolver {
            type_env,
            value_env,
            errors: Vec::new(),
            type_decls: HashMap::new(),
            type_spans: HashMap::new(),
            function_decls: HashMap::new(),
            function_spans: HashMap::new(),
        };

        // Pass 1: Collect this module's declarations; no-op on imports
        resolver.collect_declarations_for_context(source_file);

        if !resolver.errors.is_empty() {
            ctx.type_env = resolver.type_env;
            ctx.value_env = resolver.value_env;
            return Err(resolver.errors);
        }

        // Pass 2: Resolve type references and function signatures
        resolver.resolve_type_references();
        resolver.resolve_function_signatures();
        resolver.detect_circular_aliases();

        // Write back
        ctx.type_env = resolver.type_env;
        ctx.value_env = resolver.value_env;

        if resolver.errors.is_empty() {
            Ok(())
        } else {
            Err(resolver.errors)
        }
    }

    //
    // Pass 1: Collection
    //

    fn collect_declarations(&mut self, source_file: &SourceFile) {
        for item in &source_file.items {
            match item {
                Item::TypeDecl(decl) => self.collect_type_decl(decl),
                Item::Function(decl) => self.collect_function_decl(decl),
                Item::Import(import) => {
                    self.errors.push(TypeError::UnsupportedFeature {
                        feature: "import declarations",
                        span: import.span,
                        note: "Use `twk check/lower` which compiles via the module pipeline".to_string(),
                    });
                }
                Item::Stmt(_) => {
                    // Top-level statements (let bindings) are allowed
                    // They will be type-checked in check.rs, not during name resolution
                }
            }
        }
    }

    /// Like collect_declarations but treats imports as no-ops (already compiled).
    fn collect_declarations_for_context(&mut self, source_file: &SourceFile) {
        for item in &source_file.items {
            match item {
                Item::TypeDecl(decl) => self.collect_type_decl(decl),
                Item::Function(decl) => self.collect_function_decl(decl),
                Item::Import(_) => {
                    // Imports are compiled before reaching this point; no-op
                }
                Item::Stmt(_) => {}
            }
        }
    }

    fn collect_type_decl(&mut self, decl: &TypeDecl) {
        // Reject generics in Stage 2
        if !decl.type_params.is_empty() {
            self.errors.push(TypeError::GenericNotSupported {
                name: decl.name.clone(),
                span: decl.span,
                note: "Generic types will be supported in Stage 5".to_string(),
            });
            return;
        }

        // Check for duplicate type names
        if let Some(first_span) = self.type_spans.get(&decl.name) {
            self.errors.push(TypeError::DuplicateDefinition {
                name: decl.name.clone(),
                first: *first_span,
                second: decl.span,
            });
            return;
        }

        // Store the declaration for Pass 2
        self.type_spans.insert(decl.name.clone(), decl.span);
        self.type_decls.insert(decl.name.clone(), decl.clone());
    }

    fn collect_function_decl(&mut self, decl: &FunctionDecl) {
        // Reject generics in Stage 2
        if !decl.type_params.is_empty() {
            self.errors.push(TypeError::GenericNotSupported {
                name: decl.name.clone(),
                span: decl.span,
                note: "Generic functions will be supported in Stage 5".to_string(),
            });
            return;
        }

        // Check for duplicate function names
        if let Some(first_span) = self.function_spans.get(&decl.name) {
            self.errors.push(TypeError::DuplicateDefinition {
                name: decl.name.clone(),
                first: *first_span,
                second: decl.span,
            });
            return;
        }

        // Store the declaration for Pass 2
        self.function_spans.insert(decl.name.clone(), decl.span);
        self.function_decls.insert(decl.name.clone(), decl.clone());
    }

    //
    // Pass 2: Type Resolution
    //

    fn resolve_type_references(&mut self) {
        // Build TypeDefs for all declarations and add to TypeEnv
        // We need to process in dependency order, but for now we'll just iterate
        // and rely on the type lookup to work for forward references

        // Pass 2a: Collect all type names first (register them with TypeEnv)
        // Store the mapping of name -> TypeId for later updates
        let mut type_ids: HashMap<String, TypeId> = HashMap::new();
        for (name, decl) in &self.type_decls {
            // Create a placeholder TypeDef based on the variant
            let placeholder = match &decl.definition {
                AstTypeDef::Record { .. } => TypeDef::Record {
                    name: name.clone(),
                    fields: Vec::new(),
                },
                AstTypeDef::Sum { .. } => TypeDef::Sum {
                    name: name.clone(),
                    variants: Vec::new(),
                },
                AstTypeDef::Alias { .. } => TypeDef::Alias {
                    name: name.clone(),
                    target: MonoType::Void,
                },
            };
            let type_id = self.type_env.add_type(placeholder);
            type_ids.insert(name.clone(), type_id);
        }

        // Pass 2b: Resolve each type definition fully and UPDATE in place
        // This preserves TypeIds embedded in resolved MonoTypes
        let decls: Vec<TypeDecl> = self.type_decls.values().cloned().collect();

        for decl in &decls {
            if let Some(&type_id) = type_ids.get(&decl.name) {
                match self.resolve_type_def(decl) {
                    Ok(def) => {
                        // Update the existing TypeDef (preserves TypeId)
                        self.type_env.update_type(type_id, def);
                    }
                    Err(()) => {
                        // Errors already recorded in resolve_type_def
                    }
                }
            }
        }
    }

    fn resolve_type_def(&mut self, decl: &TypeDecl) -> Result<TypeDef, ()> {
        let def = match &decl.definition {
            AstTypeDef::Record { fields } => {
                let mut resolved_fields = Vec::new();
                for field in fields {
                    match self.resolve_type(&field.ty) {
                        Ok(ty) => {
                            resolved_fields.push(RecordField {
                                name: field.name.clone(),
                                ty,
                            });
                        }
                        Err(()) => {
                            // Error already recorded
                            return Err(());
                        }
                    }
                }
                TypeDef::Record {
                    name: decl.name.clone(),
                    fields: resolved_fields,
                }
            }
            AstTypeDef::Sum { variants } => {
                let mut resolved_variants = Vec::new();
                for variant in variants {
                    let mut resolved_fields = Vec::new();
                    for field_ty in &variant.fields {
                        match self.resolve_type(field_ty) {
                            Ok(ty) => resolved_fields.push(ty),
                            Err(()) => {
                                return Err(());
                            }
                        }
                    }
                    resolved_variants.push(Variant {
                        name: variant.name.clone(),
                        fields: resolved_fields,
                    });
                }
                TypeDef::Sum {
                    name: decl.name.clone(),
                    variants: resolved_variants,
                }
            }
            AstTypeDef::Alias { ty } => {
                let target = self.resolve_type(ty)?;
                TypeDef::Alias {
                    name: decl.name.clone(),
                    target,
                }
            }
        };
        Ok(def)
    }

    /// Resolve an AST type to a MonoType
    /// Delegates to TypeEnv's shared implementation
    fn resolve_type(&mut self, ty: &AstType) -> Result<MonoType, ()> {
        self.type_env.resolve_type(ty, &mut self.errors)
    }

    fn resolve_function_signatures(&mut self) {
        // Collect decls into a Vec to avoid borrowing issues
        let decls: Vec<FunctionDecl> = self.function_decls.values().cloned().collect();

        for decl in &decls {
            match self.resolve_function_sig(decl) {
                Ok(sig) => {
                    self.value_env.add_function(sig);
                }
                Err(()) => {
                    // Errors already recorded
                }
            }
        }
    }

    fn resolve_function_sig(&mut self, decl: &FunctionDecl) -> Result<FunctionSignature, ()> {
        // Resolve parameter types
        let mut params = Vec::new();
        for param in &decl.params {
            let ty = if let Some(param_ty) = &param.ty {
                self.resolve_type(param_ty)?
            } else {
                // No type annotation - not allowed in Stage 2
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "type inference for function parameters",
                    span: param.span,
                    note: "All function parameters must have type annotations in Stage 2".to_string(),
                });
                return Err(());
            };
            params.push(ty);
        }

        // Resolve return type (or None if omitted for inference)
        let ret = if let Some(ret_ty) = &decl.return_type {
            Some(self.resolve_type(ret_ty)?)
        } else {
            // No explicit return type - will be inferred from body during type checking
            None
        };

        Ok(FunctionSignature {
            name: decl.name.clone(),
            params,
            ret,
        })
    }

    //
    // Circular Alias Detection
    //

    fn detect_circular_aliases(&mut self) {
        // For each type alias, check if it eventually refers back to itself
        // Use DFS with a visited set to detect cycles

        let type_names: Vec<String> = self.type_decls.keys().cloned().collect();

        for type_name in type_names {
            let decl = match self.type_decls.get(&type_name) {
                Some(d) => d,
                None => continue,
            };

            // Only check aliases
            if !matches!(&decl.definition, AstTypeDef::Alias { .. }) {
                continue;
            }

            let mut visited = HashSet::new();
            visited.insert(type_name.clone());

            if self.is_circular_alias(&type_name, &mut visited) {
                self.errors.push(TypeError::CircularTypeAlias {
                    name: type_name.clone(),
                    span: decl.span,
                });
            }
        }
    }

    fn is_circular_alias(&self, type_name: &str, visited: &mut HashSet<String>) -> bool {
        let type_id = match self.type_env.lookup_type(type_name) {
            Some(id) => id,
            None => return false,
        };

        let def = match self.type_env.get_def(type_id) {
            Some(d) => d,
            None => return false,
        };

        match def {
            TypeDef::Alias { target, .. } => {
                // Check if target refers to a type in the visited set
                if let MonoType::Named { type_id: target_id, .. } = target {
                    if let Some(target_def) = self.type_env.get_def(*target_id) {
                        let target_name = target_def.name();
                        if visited.contains(target_name) {
                            return true; // Circular!
                        }

                        // Recursively check if the target is circular
                        visited.insert(target_name.to_string());
                        return self.is_circular_alias(target_name, visited);
                    }
                }
                false
            }
            _ => false, // Not an alias
        }
    }
}
