use crate::wasm::ir::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LinkedModuleIR {
    pub types: Vec<TypeDef>,
    pub imports: Vec<ImportDef>,
    pub funcs: Vec<FuncDef>,
    pub globals: Vec<GlobalDef>,
    pub tables: Vec<TableDef>,
    pub elems: Vec<ElemDef>,
    pub exports: Vec<ExportDef>,
    pub data: Vec<DataSegment>,
    pub start: Option<FuncSym>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LinkError {
    MissingExport {
        module: String,
        name: String,
    },
    AmbiguousExport {
        name: String,
        found_in: Vec<String>,
    },
    TypeMismatch {
        sym: FuncSym,
        expected: FuncSig,
        got: FuncSig,
    },
    NamespaceCollision {
        sym: String,
    },
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::MissingExport { module, name } => {
                write!(f, "missing export: {module}.{name}")
            }
            LinkError::AmbiguousExport { name, found_in } => {
                write!(
                    f,
                    "ambiguous export {name:?} found in: {}",
                    found_in.join(", ")
                )
            }
            LinkError::TypeMismatch { sym, .. } => {
                write!(f, "type mismatch for {sym}")
            }
            LinkError::NamespaceCollision { sym } => {
                write!(f, "namespace collision: {sym}")
            }
        }
    }
}

/// Mangle a namespace into a safe WAT identifier prefix.
/// "rt.arr" → "rt_arr", "user" → "user"
fn ns_prefix(ns: &str) -> String {
    ns.replace('.', "_")
}

/// Rename a symbol with its module namespace prefix.
fn qualify(ns: &str, sym: &str) -> String {
    format!("{}__{sym}", ns_prefix(ns))
}

/// Rewrite all `Call(sym)` instructions in a body using the rename map.
fn rewrite_calls(body: &mut Vec<Instr>, renames: &HashMap<String, String>) {
    for instr in body.iter_mut() {
        match instr {
            Instr::Call(sym) | Instr::RefFunc(sym) | Instr::ReturnCall(sym) => {
                if let Some(renamed) = renames.get(sym.as_str()) {
                    *sym = renamed.clone();
                }
            }
            Instr::If {
                then_body,
                else_body,
                ..
            } => {
                rewrite_calls(then_body, renames);
                rewrite_calls(else_body, renames);
            }
            Instr::Block { body, .. } | Instr::Loop { body, .. } => {
                rewrite_calls(body, renames);
            }
            _ => {}
        }
    }
}

/// Rewrite all `StructNew`, `StructGet`, `StructSet`, `ArrayNew`, `ArrayGet`,
/// `ArraySet`, `RefCast`, `RefNull` type references using the type rename map.
fn rewrite_type_refs(body: &mut Vec<Instr>, renames: &HashMap<String, String>) {
    for instr in body.iter_mut() {
        match instr {
            Instr::StructNew(ty)
            | Instr::StructGet(ty, _)
            | Instr::StructGetS(ty, _)
            | Instr::StructSet(ty, _)
            | Instr::ArrayNew(ty)
            | Instr::ArrayNewFixed(ty, _)
            | Instr::ArrayNewData(ty, _)
            | Instr::ArrayGet(ty)
            | Instr::ArrayGetU(ty)
            | Instr::ArraySet(ty)
            | Instr::CallIndirect { ty, .. }
            | Instr::CallRef(ty)
            | Instr::ReturnCallRef(ty) => {
                if let Some(renamed) = renames.get(ty.as_str()) {
                    *ty = renamed.clone();
                }
            }
            Instr::ArrayCopy(dst, src) => {
                if let Some(r) = renames.get(dst.as_str()) {
                    *dst = r.clone();
                }
                if let Some(r) = renames.get(src.as_str()) {
                    *src = r.clone();
                }
            }
            Instr::RefCast { heap, .. } | Instr::RefNull(heap) => {
                if let HeapType::Named(ty) = heap {
                    if let Some(renamed) = renames.get(ty.as_str()) {
                        *ty = renamed.clone();
                    }
                }
            }
            Instr::If {
                then_body,
                else_body,
                ..
            } => {
                rewrite_type_refs(then_body, renames);
                rewrite_type_refs(else_body, renames);
            }
            Instr::Block { body, .. } | Instr::Loop { body, .. } => {
                rewrite_type_refs(body, renames);
            }
            _ => {}
        }
    }
}

fn rewrite_val_type(vt: &mut ValType, renames: &HashMap<String, String>) {
    if let ValType::Ref {
        heap: HeapType::Named(ty),
        ..
    } = vt
    {
        if let Some(renamed) = renames.get(ty.as_str()) {
            *ty = renamed.clone();
        }
    }
}

pub fn link(
    modules: Vec<ModuleIR>,
    entry: Option<FuncSym>,
) -> Result<LinkedModuleIR, Vec<LinkError>> {
    let mut errors: Vec<LinkError> = Vec::new();

    // Build export map: (namespace, wasm_name) → qualified func sym
    // Maps what each module exports by its wasm_name to the qualified sym
    let mut export_map: HashMap<(String, String), String> = HashMap::new();
    for module in &modules {
        let ns = &module.namespace;
        for exp in &module.exports {
            let key = (ns.clone(), exp.wasm_name.clone());
            export_map.insert(key, qualify(ns, &exp.func_sym));
        }
    }

    // Build func rename maps (original sym → qualified sym) per module
    // Also build type rename maps
    let mut all_func_renames: Vec<HashMap<String, String>> = Vec::new();
    let mut all_type_renames: Vec<HashMap<String, String>> = Vec::new();

    for module in &modules {
        let ns = &module.namespace;
        let mut func_renames: HashMap<String, String> = HashMap::new();
        let mut type_renames: HashMap<String, String> = HashMap::new();

        for func in &module.funcs {
            let qualified = qualify(ns, &func.name);
            func_renames.insert(func.name.clone(), qualified);
        }
        for imp in &module.imports {
            // imports keep their as_sym (resolved later)
            let qualified = qualify(ns, &imp.as_sym);
            func_renames.insert(imp.as_sym.clone(), qualified);
        }
        for td in &module.types {
            let qualified = qualify(ns, td.name());
            type_renames.insert(td.name().to_string(), qualified);
        }

        all_func_renames.push(func_renames);
        all_type_renames.push(type_renames);
    }

    // Resolve inter-module imports: build a global call redirect map
    // import_redirect: qualified_import_sym → resolved_export_sym
    let mut import_redirects: Vec<HashMap<String, String>> = Vec::new();

    for (mod_idx, module) in modules.iter().enumerate() {
        let ns = &module.namespace;
        let mut redirects: HashMap<String, String> = HashMap::new();

        for imp in &module.imports {
            if imp.module == "host" {
                // Keep host imports — no redirect
                continue;
            }
            let key = (imp.module.clone(), imp.name.clone());
            match export_map.get(&key) {
                Some(resolved_sym) => {
                    let local_qualified = qualify(ns, &imp.as_sym);
                    redirects.insert(local_qualified, resolved_sym.clone());
                }
                None => {
                    errors.push(LinkError::MissingExport {
                        module: imp.module.clone(),
                        name: imp.name.clone(),
                    });
                }
            }
        }

        // Also redirect from unqualified import sym to resolved
        for imp in &module.imports {
            if imp.module == "host" {
                continue;
            }
            let key = (imp.module.clone(), imp.name.clone());
            if let Some(resolved_sym) = export_map.get(&key) {
                redirects.insert(imp.as_sym.clone(), resolved_sym.clone());
            }
        }

        let _ = mod_idx;
        import_redirects.push(redirects);
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // Now merge everything
    let mut merged_types: Vec<TypeDef> = Vec::new();
    let mut merged_imports: Vec<ImportDef> = Vec::new();
    let mut merged_funcs: Vec<FuncDef> = Vec::new();
    let mut merged_globals: Vec<GlobalDef> = Vec::new();
    let mut merged_tables: Vec<TableDef> = Vec::new();
    let mut merged_elems: Vec<ElemDef> = Vec::new();
    let mut merged_exports: Vec<ExportDef> = Vec::new();
    let mut merged_data: Vec<DataSegment> = Vec::new();
    let mut start_funcs: Vec<FuncSym> = Vec::new();

    for (mod_idx, module) in modules.into_iter().enumerate() {
        let ns = &module.namespace;
        let func_renames = &all_func_renames[mod_idx];
        let type_renames = &all_type_renames[mod_idx];
        let redirects = &import_redirects[mod_idx];

        // Merge types (qualified names)
        for td in module.types {
            let renamed = match td {
                TypeDef::Struct { name, mut fields } => {
                    for field in &mut fields {
                        rewrite_val_type(&mut field.ty, type_renames);
                    }
                    TypeDef::Struct {
                        name: qualify(ns, &name),
                        fields,
                    }
                }
                TypeDef::Array { name, mut elem } => {
                    rewrite_val_type(&mut elem.ty, type_renames);
                    TypeDef::Array {
                        name: qualify(ns, &name),
                        elem,
                    }
                }
                TypeDef::FuncType {
                    name,
                    mut params,
                    mut results,
                } => {
                    for p in &mut params {
                        rewrite_val_type(p, type_renames);
                    }
                    for r in &mut results {
                        rewrite_val_type(r, type_renames);
                    }
                    TypeDef::FuncType {
                        name: qualify(ns, &name),
                        params,
                        results,
                    }
                }
            };
            merged_types.push(renamed);
        }

        // Merge host imports (qualified names)
        for imp in module.imports {
            if imp.module != "host" {
                continue;
            }
            merged_imports.push(ImportDef {
                as_sym: qualify(ns, &imp.as_sym),
                ..imp
            });
        }

        // Build combined rename once per module: func renames + import redirects.
        // Used by both func bodies and global initialisers.
        let mut combined = func_renames.clone();
        for (k, v) in redirects {
            combined.insert(k.clone(), v.clone());
        }

        // Merge funcs
        for mut func in module.funcs {
            rewrite_calls(&mut func.body, &combined);
            rewrite_type_refs(&mut func.body, type_renames);

            // Rewrite param/result types
            for p in &mut func.params {
                rewrite_val_type(p, type_renames);
            }
            for r in &mut func.results {
                rewrite_val_type(r, type_renames);
            }
            for l in &mut func.locals {
                rewrite_val_type(l, type_renames);
            }

            merged_funcs.push(FuncDef {
                name: qualify(ns, &func.name),
                params: func.params,
                results: func.results,
                locals: func.locals,
                body: func.body,
            });
        }

        // Merge globals
        for mut global in module.globals {
            rewrite_calls(&mut global.init, &combined);
            rewrite_type_refs(&mut global.init, type_renames);
            merged_globals.push(GlobalDef {
                name: qualify(ns, &global.name),
                ..global
            });
        }

        // Merge tables
        for table in module.tables {
            merged_tables.push(TableDef {
                name: qualify(ns, &table.name),
                ..table
            });
        }

        // Merge elems
        for mut elem in module.elems {
            let table_name = qualify(ns, &elem.table);
            elem.funcs = elem
                .funcs
                .into_iter()
                .map(|f| func_renames.get(&f).cloned().unwrap_or(f))
                .collect();
            merged_elems.push(ElemDef {
                table: table_name,
                offset: elem.offset,
                funcs: elem.funcs,
            });
        }

        // Merge exports
        for exp in module.exports {
            let resolved_sym = func_renames
                .get(&exp.func_sym)
                .cloned()
                .unwrap_or_else(|| qualify(ns, &exp.func_sym));
            merged_exports.push(ExportDef {
                wasm_name: exp.wasm_name,
                func_sym: resolved_sym,
            });
        }

        // Merge data
        for data in module.data {
            merged_data.push(DataSegment {
                name: qualify(ns, &data.name),
                ..data
            });
        }

        // Collect start functions
        if let Some(start) = module.start {
            start_funcs.push(qualify(ns, &start));
        }
    }

    // Synthesize __linked_init if there are start functions or an entry point
    let final_start = if !start_funcs.is_empty() || entry.is_some() {
        let mut init_body: Vec<Instr> = start_funcs.into_iter().map(|s| Instr::Call(s)).collect();
        if let Some(entry_sym) = entry {
            init_body.push(Instr::Call(entry_sym));
        }
        merged_funcs.push(FuncDef {
            name: "__linked_init".into(),
            params: Vec::new(),
            results: Vec::new(),
            locals: Vec::new(),
            body: init_body,
        });
        Some("__linked_init".into())
    } else {
        None
    };

    Ok(LinkedModuleIR {
        types: merged_types,
        imports: merged_imports,
        funcs: merged_funcs,
        globals: merged_globals,
        tables: merged_tables,
        elems: merged_elems,
        exports: merged_exports,
        data: merged_data,
        start: final_start,
    })
}
