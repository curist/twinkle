use super::env::{TypeEnv, ValueEnv};
use super::error::TypeError;
use super::ty::{
    FunctionSignature, MonoType, RecordField, TypeDef, TypeId, Variant, method_receiver_type_id,
};
use crate::module::artifacts::ResolvedModule;
use crate::syntax::ast::{
    FunctionDecl, Item, SourceFile, Type as AstType, TypeDecl, TypeDef as AstTypeDef,
};
use crate::syntax::span::Span;
use std::collections::{HashMap, HashSet};

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
    type_decl_order: Vec<String>,

    // Track function declarations for Pass 2
    function_decls: HashMap<String, FunctionDecl>,
    function_spans: HashMap<String, Span>,
    function_decl_order: Vec<String>,

    // TypeIds defined in this module — only these are eligible for inherent methods
    local_type_ids: HashSet<TypeId>,

    // Whether this is an internal (stdlib/prelude) module — allowed to register
    // methods on builtin types.
    is_internal: bool,
}

impl Resolver {
    /// Resolve all names in a source file.
    ///
    /// Takes accumulated `type_env` and `value_env` from previously compiled
    /// dependencies (pass `TypeEnv::new()` / `ValueEnv::new()` for single-module use).
    /// Adds this module's declarations and returns the updated environments.
    pub fn resolve(
        source_file: &SourceFile,
        type_env: TypeEnv,
        value_env: ValueEnv,
    ) -> Result<ResolvedModule, Vec<TypeError>> {
        Self::resolve_with_options(source_file, type_env, value_env, false)
    }

    /// Resolve with options. `is_internal` allows stdlib/prelude modules to
    /// register methods on builtin types.
    pub fn resolve_with_options(
        source_file: &SourceFile,
        type_env: TypeEnv,
        value_env: ValueEnv,
        is_internal: bool,
    ) -> Result<ResolvedModule, Vec<TypeError>> {
        let mut resolver = Resolver {
            type_env,
            value_env,
            errors: Vec::new(),
            type_decls: HashMap::new(),
            type_spans: HashMap::new(),
            type_decl_order: Vec::new(),
            function_decls: HashMap::new(),
            function_spans: HashMap::new(),
            function_decl_order: Vec::new(),
            local_type_ids: HashSet::new(),
            is_internal,
        };

        // Pass 1: Collect this module's declarations; imports are no-ops (already compiled)
        resolver.collect_declarations_for_context(source_file);

        if !resolver.errors.is_empty() {
            return Err(resolver.errors);
        }

        // Pass 2: Resolve type references and build environments
        resolver.resolve_type_references();
        resolver.resolve_function_signatures();
        resolver.detect_circular_aliases();

        if !resolver.errors.is_empty() {
            Err(resolver.errors)
        } else {
            Ok(ResolvedModule {
                type_env: resolver.type_env,
                value_env: resolver.value_env,
            })
        }
    }

    //
    // Pass 1: Collection
    //

    /// Collect this module's declarations; imports are no-ops (already compiled by caller).
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
        self.type_decl_order.push(decl.name.clone());
    }

    fn collect_function_decl(&mut self, decl: &FunctionDecl) {
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
        self.function_decl_order.push(decl.name.clone());
    }

    //
    // Pass 2: Type Resolution
    //

    fn resolve_type_references(&mut self) {
        // Build TypeDefs for all declarations and add to TypeEnv
        // Preserve source declaration order so TypeId assignment is stable.
        let decls: Vec<TypeDecl> = self
            .type_decl_order
            .iter()
            .filter_map(|name| self.type_decls.get(name).cloned())
            .collect();

        // Pass 2a: Collect all type names first (register them with TypeEnv)
        // Store the mapping of name -> TypeId for later updates
        let mut type_ids: HashMap<String, TypeId> = HashMap::new();
        for decl in &decls {
            let name = &decl.name;
            // Create a placeholder TypeDef based on the variant
            // Include type_params so arity checks work during Pass 2b resolution
            let placeholder = match &decl.definition {
                AstTypeDef::Record { .. } => TypeDef::Record {
                    name: name.clone(),
                    type_params: decl.type_params.clone(),
                    fields: Vec::new(),
                    doc: decl.doc.clone(),
                },
                AstTypeDef::Sum { .. } => TypeDef::Sum {
                    name: name.clone(),
                    type_params: decl.type_params.clone(),
                    variants: Vec::new(),
                    doc: decl.doc.clone(),
                },
                AstTypeDef::Alias { .. } => TypeDef::Alias {
                    name: name.clone(),
                    type_params: decl.type_params.clone(),
                    target: MonoType::Void,
                    doc: decl.doc.clone(),
                },
            };
            let type_id = self.type_env.add_type(placeholder);
            type_ids.insert(name.clone(), type_id);
            self.local_type_ids.insert(type_id);
        }

        // Pass 2b: Resolve each type definition fully and UPDATE in place
        // This preserves TypeIds embedded in resolved MonoTypes
        //
        // Non-alias types (records, sums) are resolved first since they have no
        // ordering dependencies. Aliases are then resolved in topological order
        // so that alias chains (e.g. A -> B -> C -> Record) work regardless of
        // declaration order.
        let (alias_decls, non_alias_decls): (Vec<&TypeDecl>, Vec<&TypeDecl>) = decls
            .iter()
            .partition(|d| matches!(d.definition, AstTypeDef::Alias { .. }));

        // First: resolve records and sums
        for decl in &non_alias_decls {
            if let Some(&type_id) = type_ids.get(&decl.name) {
                match self.resolve_type_def(decl) {
                    Ok(def) => {
                        self.type_env.update_type(type_id, def);
                    }
                    Err(()) => {}
                }
            }
        }

        // Then: resolve aliases in topological (dependency) order
        let alias_names: HashSet<&str> = alias_decls.iter().map(|d| d.name.as_str()).collect();
        let sorted_aliases = topo_sort_aliases(&alias_decls, &alias_names);
        for decl in sorted_aliases {
            if let Some(&type_id) = type_ids.get(&decl.name) {
                match self.resolve_type_def(decl) {
                    Ok(def) => {
                        self.type_env.update_type(type_id, def);
                    }
                    Err(()) => {}
                }
            }
        }
    }

    fn resolve_type_def(&mut self, decl: &TypeDecl) -> Result<TypeDef, ()> {
        let type_params = &decl.type_params;
        let def = match &decl.definition {
            AstTypeDef::Record { fields } => {
                let mut resolved_fields = Vec::new();
                for field in fields {
                    match self.resolve_type_with_vars(&field.ty, type_params) {
                        Ok(ty) => {
                            resolved_fields.push(RecordField {
                                name: field.name.clone(),
                                ty,
                            });
                        }
                        Err(()) => {
                            return Err(());
                        }
                    }
                }
                TypeDef::Record {
                    name: decl.name.clone(),
                    type_params: type_params.clone(),
                    fields: resolved_fields,
                    doc: decl.doc.clone(),
                }
            }
            AstTypeDef::Sum { variants } => {
                let mut resolved_variants = Vec::new();
                for variant in variants {
                    let mut resolved_fields = Vec::new();
                    for field_ty in &variant.fields {
                        match self.resolve_type_with_vars(field_ty, type_params) {
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
                    type_params: type_params.clone(),
                    variants: resolved_variants,
                    doc: decl.doc.clone(),
                }
            }
            AstTypeDef::Alias { ty } => {
                let target = self.resolve_type_with_vars(ty, type_params)?;
                TypeDef::Alias {
                    name: decl.name.clone(),
                    type_params: type_params.clone(),
                    target,
                    doc: decl.doc.clone(),
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
        // Collect decls in source order to keep signature registration deterministic.
        let decls: Vec<FunctionDecl> = self
            .function_decl_order
            .iter()
            .filter_map(|name| self.function_decls.get(name).cloned())
            .collect();

        for decl in &decls {
            match self.resolve_function_sig(decl) {
                Ok(sig) => {
                    // Register inherent methods only for types owned by this module.
                    // Internal (stdlib/prelude) modules may also register methods
                    // on builtin types.
                    if let Some(receiver_ty) = sig.params.first() {
                        if let Some(type_id) = method_receiver_type_id(receiver_ty) {
                            let is_local = self.local_type_ids.contains(&type_id);
                            let is_builtin_allowed = self.is_internal && !is_local;
                            if is_local || is_builtin_allowed {
                                self.type_env.add_method(
                                    type_id,
                                    sig.name.clone(),
                                    sig.name.clone(),
                                );
                            }
                        }
                    }
                    self.value_env.add_function(sig);
                }
                Err(()) => {
                    // Errors already recorded
                }
            }
        }
    }

    fn resolve_function_sig(&mut self, decl: &FunctionDecl) -> Result<FunctionSignature, ()> {
        let type_params = decl.type_params.clone();

        // Resolve parameter types (type param names resolve to Var)
        let mut params = Vec::new();
        for param in &decl.params {
            let ty = if let Some(param_ty) = &param.ty {
                self.resolve_type_with_vars(param_ty, &type_params)?
            } else {
                self.errors.push(TypeError::UnsupportedFeature {
                    feature: "type inference for function parameters",
                    span: param.span,
                    note: "Function declaration parameters must have type annotations".to_string(),
                });
                return Err(());
            };
            params.push(ty);
        }

        // Resolve return type (or None if omitted for inference)
        let ret = if let Some(ret_ty) = &decl.return_type {
            Some(self.resolve_type_with_vars(ret_ty, &type_params)?)
        } else {
            None
        };

        Ok(FunctionSignature {
            name: decl.name.clone(),
            type_params,
            param_names: decl.params.iter().map(|param| param.name.clone()).collect(),
            params,
            ret,
            doc: decl.doc.clone(),
        })
    }

    fn resolve_type_with_vars(
        &mut self,
        ty: &AstType,
        type_vars: &[String],
    ) -> Result<MonoType, ()> {
        // If this is a bare name that matches a type variable, return Var(name)
        if let AstType::Named { name, args, .. } = ty {
            if args.is_empty() && type_vars.contains(name) {
                return Ok(MonoType::Var(name.clone()));
            }
        }
        // Recursively handle compound types with type vars
        match ty {
            AstType::Named { name, args, span } if !args.is_empty() => {
                // Try built-in generic types (Array, Dict, etc.) with var-aware arg resolution
                let resolved_args: Vec<MonoType> = args
                    .iter()
                    .map(|a| self.resolve_type_with_vars(a, type_vars))
                    .collect::<Result<_, _>>()?;
                // Re-use env's logic by building a synthetic type with resolved args
                // For known built-ins, handle directly
                match name.as_str() {
                    "Vector" if resolved_args.len() == 1 => Ok(MonoType::Vector(Box::new(
                        resolved_args.into_iter().next().unwrap(),
                    ))),
                    "Dict" if resolved_args.len() == 2 => {
                        let mut it = resolved_args.into_iter();
                        Ok(MonoType::Dict(
                            Box::new(it.next().unwrap()),
                            Box::new(it.next().unwrap()),
                        ))
                    }
                    "Option" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: crate::types::ty::OPTION_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Result" if resolved_args.len() == 2 => Ok(MonoType::Named {
                        type_id: crate::types::ty::RESULT_TYPE_ID,
                        args: resolved_args,
                    }),
                    "Cell" if resolved_args.len() == 1 => Ok(MonoType::Named {
                        type_id: crate::types::ty::CELL_TYPE_ID,
                        args: resolved_args,
                    }),
                    _ => {
                        // User-defined generic type: look up TypeId and use pre-resolved args
                        match self.type_env.lookup_type(name) {
                            Some(type_id) => {
                                let expected_arity = self
                                    .type_env
                                    .get_def(type_id)
                                    .map(|d| d.type_params().len())
                                    .unwrap_or(0);
                                if resolved_args.len() != expected_arity {
                                    self.errors.push(TypeError::UndefinedType {
                                        name: format!(
                                            "{} (expected {} type arg(s), found {})",
                                            name,
                                            expected_arity,
                                            resolved_args.len()
                                        ),
                                        span: *span,
                                    });
                                    Err(())
                                } else {
                                    Ok(MonoType::Named {
                                        type_id,
                                        args: resolved_args,
                                    })
                                }
                            }
                            None => {
                                self.errors.push(TypeError::UndefinedType {
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
                let param_tys: Vec<MonoType> = params
                    .iter()
                    .map(|p| self.resolve_type_with_vars(p, type_vars))
                    .collect::<Result<_, _>>()?;
                let ret_ty = self.resolve_type_with_vars(ret, type_vars)?;
                Ok(MonoType::Function {
                    params: param_tys,
                    ret: Box::new(ret_ty),
                })
            }
            _ => self.resolve_type(ty),
        }
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
                if let MonoType::Named {
                    type_id: target_id, ..
                } = target
                {
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

/// Collect all type names referenced by an AST type annotation.
fn collect_type_refs<'a>(ty: &'a AstType, out: &mut Vec<&'a str>) {
    match ty {
        AstType::Named { name, args, .. } => {
            out.push(name.as_str());
            for arg in args {
                collect_type_refs(arg, out);
            }
        }
        AstType::Function { params, ret, .. } => {
            for p in params {
                collect_type_refs(p, out);
            }
            collect_type_refs(ret, out);
        }
    }
}

/// Topological sort of alias declarations by their dependencies on other aliases.
/// Circular aliases are already rejected by `detect_circular_aliases`, so this is a DAG.
fn topo_sort_aliases<'a>(
    alias_decls: &[&'a TypeDecl],
    alias_names: &HashSet<&str>,
) -> Vec<&'a TypeDecl> {
    // Build adjacency: alias name -> set of alias names it depends on
    let mut deps: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut decl_map: HashMap<&str, &'a TypeDecl> = HashMap::new();

    for decl in alias_decls {
        decl_map.insert(decl.name.as_str(), decl);
        if let AstTypeDef::Alias { ty } = &decl.definition {
            let mut refs = Vec::new();
            collect_type_refs(ty, &mut refs);
            let alias_deps: Vec<&str> = refs
                .into_iter()
                .filter(|r| alias_names.contains(r) && *r != decl.name.as_str())
                .collect();
            deps.insert(decl.name.as_str(), alias_deps);
        }
    }

    // Kahn's algorithm
    let mut in_degree: HashMap<&str, usize> =
        alias_decls.iter().map(|d| (d.name.as_str(), 0)).collect();
    for (_, dep_list) in &deps {
        for dep in dep_list {
            if let Some(count) = in_degree.get_mut(dep) {
                *count += 1;
            }
        }
    }

    // Note: edges point from dependency TO dependent (dep_list are prerequisites),
    // but in_degree counts how many times a node appears as a dependency.
    // We need reverse adjacency: for each dep, which aliases depend on it.
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    for (alias, dep_list) in &deps {
        for dep in dep_list {
            reverse.entry(*dep).or_default().push(*alias);
        }
    }

    // Recompute in_degree correctly: count prerequisites
    for (name, _) in &decl_map {
        in_degree.insert(*name, deps.get(name).map_or(0, |d| d.len()));
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(&name, _)| name)
        .collect();
    queue.sort(); // deterministic order for aliases with no deps

    let mut result: Vec<&'a TypeDecl> = Vec::with_capacity(alias_decls.len());
    while let Some(name) = queue.pop() {
        if let Some(decl) = decl_map.get(name) {
            result.push(decl);
        }
        if let Some(dependents) = reverse.get(name) {
            for dependent in dependents {
                if let Some(count) = in_degree.get_mut(dependent) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push(dependent);
                    }
                }
            }
        }
    }

    result
}
