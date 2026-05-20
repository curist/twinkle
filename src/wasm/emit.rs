use crate::wasm::ir::*;
use crate::wasm::linker::LinkedModuleIR;

pub fn emit_wat(module: &LinkedModuleIR) -> String {
    let mut out = String::new();
    out.push_str("(module\n");

    // 1. Type definitions
    for td in &module.types {
        out.push_str(&emit_typedef(td));
        out.push('\n');
    }

    // Collect deduplicated func types for function defs and imports
    let mut func_type_registry: Vec<(Vec<ValType>, Vec<ValType>)> = Vec::new();
    let get_or_insert = |registry: &mut Vec<(Vec<ValType>, Vec<ValType>)>,
                         params: &[ValType],
                         results: &[ValType]|
     -> usize {
        if let Some(idx) = registry
            .iter()
            .position(|(p, r)| p == params && r == results)
        {
            idx
        } else {
            registry.push((params.to_vec(), results.to_vec()));
            registry.len() - 1
        }
    };

    // Pre-register all func signatures
    for imp in &module.imports {
        get_or_insert(&mut func_type_registry, &imp.params, &imp.results);
    }
    for func in &module.funcs {
        get_or_insert(&mut func_type_registry, &func.params, &func.results);
    }

    // Emit deduplicated func types
    for (idx, (params, results)) in func_type_registry.iter().enumerate() {
        let mut s = format!("  (type $functype_{idx} (func");
        if !params.is_empty() {
            s.push_str(" (param");
            for p in params {
                s.push(' ');
                s.push_str(&emit_val_type(p));
            }
            s.push(')');
        }
        if !results.is_empty() {
            s.push_str(" (result");
            for r in results {
                s.push(' ');
                s.push_str(&emit_val_type(r));
            }
            s.push(')');
        }
        s.push_str("))\n");
        out.push_str(&s);
    }

    // 2. Import declarations
    for imp in &module.imports {
        let type_idx = func_type_registry
            .iter()
            .position(|(p, r)| p == &imp.params && r == &imp.results)
            .unwrap();
        out.push_str(&format!(
            "  (import {:?} {:?} (func ${} (type $functype_{type_idx})))\n",
            imp.module, imp.name, imp.as_sym
        ));
    }

    // 3. Table declarations
    for table in &module.tables {
        let max_str = match table.max {
            Some(m) => format!(" {m}"),
            None => String::new(),
        };
        out.push_str(&format!(
            "  (table ${} {} {}{} {})\n",
            table.name,
            table.min,
            max_str,
            "",
            emit_val_type(&table.elem_ty)
        ));
    }

    // 4. Global declarations
    for global in &module.globals {
        let mut_str = if global.mutable { "(mut " } else { "" };
        let mut_end = if global.mutable { ")" } else { "" };
        let init_str: String = global
            .init
            .iter()
            .map(|i| emit_instr(i, 0))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "  (global ${} {}{}{} {})\n",
            global.name,
            mut_str,
            emit_val_type(&global.ty),
            mut_end,
            init_str
        ));
    }

    // 5. Function definitions
    for func in &module.funcs {
        let type_idx = func_type_registry
            .iter()
            .position(|(p, r)| p == &func.params && r == &func.results)
            .unwrap();
        out.push_str(&emit_func(func, type_idx));
        out.push('\n');
    }

    // 6. Element segments
    for elem in &module.elems {
        let offset_str: String = elem
            .offset
            .iter()
            .map(|i| emit_instr(i, 0))
            .collect::<Vec<_>>()
            .join(" ");
        let funcs_str: String = elem
            .funcs
            .iter()
            .map(|f| format!("${f}"))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "  (elem (table ${}) ({offset_str}) func {funcs_str})\n",
            elem.table
        ));
    }

    // 6b. Declarative element segment for ref.func references
    let ref_func_syms = collect_ref_func_syms(module);
    if !ref_func_syms.is_empty() {
        let funcs_str = ref_func_syms
            .iter()
            .map(|f| format!("${f}"))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!("  (elem declare func {funcs_str})\n"));
    }

    // 7. Data segments
    for data in &module.data {
        let bytes_str: String = data
            .bytes
            .iter()
            .map(|b| format!("\\{b:02x}"))
            .collect::<String>();
        if data.offset.is_empty() {
            // passive
            out.push_str(&format!("  (data ${} \"{bytes_str}\")\n", data.name));
        } else {
            let offset_str: String = data
                .offset
                .iter()
                .map(|i| emit_instr(i, 0))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!(
                "  (data ${} ({offset_str}) \"{bytes_str}\")\n",
                data.name
            ));
        }
    }

    // 8. Export declarations
    for exp in &module.exports {
        out.push_str(&format!(
            "  (export {:?} (func ${}))\n",
            exp.wasm_name, exp.func_sym
        ));
    }

    // 9. Start
    if let Some(start) = &module.start {
        out.push_str(&format!("  (start ${start})\n"));
    }

    out.push(')');
    out
}

/// Collect all function symbols referenced by `ref.func` instructions.
fn collect_ref_func_syms(module: &LinkedModuleIR) -> Vec<String> {
    let mut syms = std::collections::BTreeSet::new();
    for func in &module.funcs {
        collect_ref_func_from_body(&func.body, &mut syms);
    }
    syms.into_iter().collect()
}

fn collect_ref_func_from_body(body: &[Instr], syms: &mut std::collections::BTreeSet<String>) {
    for instr in body {
        match instr {
            Instr::RefFunc(sym) => {
                syms.insert(sym.clone());
            }
            Instr::If {
                then_body,
                else_body,
                ..
            } => {
                collect_ref_func_from_body(then_body, syms);
                collect_ref_func_from_body(else_body, syms);
            }
            Instr::Block { body, .. } | Instr::Loop { body, .. } => {
                collect_ref_func_from_body(body, syms);
            }
            _ => {}
        }
    }
}

pub fn emit_val_type(vt: &ValType) -> String {
    match vt {
        ValType::I8 => "i8".into(),
        ValType::I32 => "i32".into(),
        ValType::I64 => "i64".into(),
        ValType::F32 => "f32".into(),
        ValType::F64 => "f64".into(),
        ValType::Anyref => "anyref".into(),
        ValType::I31ref => "i31ref".into(),
        ValType::Funcref => "funcref".into(),
        ValType::Ref { nullable, heap } => {
            let ht = emit_heap_type(heap);
            if *nullable {
                format!("(ref null {ht})")
            } else {
                format!("(ref {ht})")
            }
        }
    }
}

pub fn emit_heap_type(ht: &HeapType) -> String {
    match ht {
        HeapType::Named(name) => format!("${name}"),
        HeapType::Any => "any".into(),
        HeapType::Eq => "eq".into(),
        HeapType::I31 => "i31".into(),
        HeapType::Func => "func".into(),
        HeapType::None => "none".into(),
        HeapType::Extern => "extern".into(),
    }
}

pub fn emit_typedef(td: &TypeDef) -> String {
    match td {
        TypeDef::Struct {
            name,
            fields,
            supertype,
            non_final,
        } => {
            let has_sub = supertype.is_some() || *non_final;
            let mut s = if let Some(parent) = supertype {
                format!("  (type ${name} (sub ${parent} (struct")
            } else if *non_final {
                format!("  (type ${name} (sub (struct")
            } else {
                format!("  (type ${name} (struct")
            };
            for field in fields {
                s.push_str(" (field");
                if let Some(n) = &field.name {
                    s.push_str(&format!(" ${n}"));
                }
                if field.mutable {
                    s.push_str(&format!(" (mut {})", emit_val_type(&field.ty)));
                } else {
                    s.push(' ');
                    s.push_str(&emit_val_type(&field.ty));
                }
                s.push(')');
            }
            if has_sub {
                s.push_str(")))"); // close struct, sub, type
            } else {
                s.push_str("))"); // close struct, type
            }
            s
        }
        TypeDef::Array { name, elem } => {
            let mut s = format!("  (type ${name} (array");
            if elem.mutable {
                s.push_str(&format!(" (mut {})", emit_val_type(&elem.ty)));
            } else {
                s.push(' ');
                s.push_str(&emit_val_type(&elem.ty));
            }
            s.push_str("))");
            s
        }
        TypeDef::FuncType {
            name,
            params,
            results,
        } => {
            let mut s = format!("  (type ${name} (func");
            if !params.is_empty() {
                s.push_str(" (param");
                for p in params {
                    s.push(' ');
                    s.push_str(&emit_val_type(p));
                }
                s.push(')');
            }
            if !results.is_empty() {
                s.push_str(" (result");
                for r in results {
                    s.push(' ');
                    s.push_str(&emit_val_type(r));
                }
                s.push(')');
            }
            s.push_str("))");
            s
        }
    }
}

pub fn emit_func(func: &FuncDef, type_idx: usize) -> String {
    let mut s = format!("  (func ${} (type $functype_{type_idx})", func.name);

    // Params
    for (i, p) in func.params.iter().enumerate() {
        s.push_str(&format!("\n    (param $p{i} {})", emit_val_type(p)));
    }

    // Results
    for r in &func.results {
        s.push_str(&format!("\n    (result {})", emit_val_type(r)));
    }

    // Locals (same $p{N} naming as params — they share the index space)
    let param_count = func.params.len() as u32;
    for (i, l) in func.locals.iter().enumerate() {
        let local_idx = param_count + i as u32;
        s.push_str(&format!("\n    (local $p{local_idx} {})", emit_val_type(l)));
    }

    // Body
    for instr in &func.body {
        s.push('\n');
        s.push_str(&emit_instr(instr, 4));
    }

    s.push_str("\n  )");
    s
}

pub fn emit_instr(instr: &Instr, indent: usize) -> String {
    let pad = " ".repeat(indent);
    match instr {
        Instr::LocalGet(i) => format!("{pad}local.get $p{i}"),
        Instr::LocalSet(i) => format!("{pad}local.set $p{i}"),
        Instr::LocalTee(i) => format!("{pad}local.tee $p{i}"),
        Instr::GlobalGet(sym) => format!("{pad}global.get ${sym}"),
        Instr::GlobalSet(sym) => format!("{pad}global.set ${sym}"),

        Instr::I32Const(v) => format!("{pad}i32.const {v}"),
        Instr::I64Const(v) => format!("{pad}i64.const {v}"),
        Instr::F64Const(v) => {
            if v.is_nan() {
                format!("{pad}f64.const nan")
            } else if v.is_infinite() {
                if v.is_sign_positive() {
                    format!("{pad}f64.const inf")
                } else {
                    format!("{pad}f64.const -inf")
                }
            } else {
                format!("{pad}f64.const {v}")
            }
        }

        Instr::I32Add => format!("{pad}i32.add"),
        Instr::I32Sub => format!("{pad}i32.sub"),
        Instr::I32Mul => format!("{pad}i32.mul"),
        Instr::I32DivS => format!("{pad}i32.div_s"),
        Instr::I32RemS => format!("{pad}i32.rem_s"),
        Instr::I32And => format!("{pad}i32.and"),
        Instr::I32Or => format!("{pad}i32.or"),
        Instr::I32Xor => format!("{pad}i32.xor"),
        Instr::I32Shl => format!("{pad}i32.shl"),
        Instr::I32ShrU => format!("{pad}i32.shr_u"),
        Instr::I32ShrS => format!("{pad}i32.shr_s"),
        Instr::I32Eq => format!("{pad}i32.eq"),
        Instr::I32Ne => format!("{pad}i32.ne"),
        Instr::I32LtS => format!("{pad}i32.lt_s"),
        Instr::I32GtS => format!("{pad}i32.gt_s"),
        Instr::I32LeS => format!("{pad}i32.le_s"),
        Instr::I32GeS => format!("{pad}i32.ge_s"),
        Instr::I32LtU => format!("{pad}i32.lt_u"),
        Instr::I32GtU => format!("{pad}i32.gt_u"),
        Instr::I32LeU => format!("{pad}i32.le_u"),
        Instr::I32GeU => format!("{pad}i32.ge_u"),
        Instr::I32Eqz => format!("{pad}i32.eqz"),

        Instr::I64Add => format!("{pad}i64.add"),
        Instr::I64Sub => format!("{pad}i64.sub"),
        Instr::I64Mul => format!("{pad}i64.mul"),
        Instr::I64DivS => format!("{pad}i64.div_s"),
        Instr::I64RemS => format!("{pad}i64.rem_s"),
        Instr::I64And => format!("{pad}i64.and"),
        Instr::I64Or => format!("{pad}i64.or"),
        Instr::I64Xor => format!("{pad}i64.xor"),
        Instr::I64Shl => format!("{pad}i64.shl"),
        Instr::I64ShrS => format!("{pad}i64.shr_s"),
        Instr::I64ShrU => format!("{pad}i64.shr_u"),
        Instr::I64Eq => format!("{pad}i64.eq"),
        Instr::I64Ne => format!("{pad}i64.ne"),
        Instr::I64LtS => format!("{pad}i64.lt_s"),
        Instr::I64GtS => format!("{pad}i64.gt_s"),
        Instr::I64LeS => format!("{pad}i64.le_s"),
        Instr::I64GeS => format!("{pad}i64.ge_s"),
        Instr::I64Eqz => format!("{pad}i64.eqz"),

        Instr::F64Add => format!("{pad}f64.add"),
        Instr::F64Sub => format!("{pad}f64.sub"),
        Instr::F64Mul => format!("{pad}f64.mul"),
        Instr::F64Div => format!("{pad}f64.div"),
        Instr::F64Neg => format!("{pad}f64.neg"),
        Instr::F64Eq => format!("{pad}f64.eq"),
        Instr::F64Ne => format!("{pad}f64.ne"),
        Instr::F64Lt => format!("{pad}f64.lt"),
        Instr::F64Gt => format!("{pad}f64.gt"),
        Instr::F64Le => format!("{pad}f64.le"),
        Instr::F64Ge => format!("{pad}f64.ge"),

        Instr::I64ExtendI32S => format!("{pad}i64.extend_i32_s"),
        Instr::I64ExtendI32U => format!("{pad}i64.extend_i32_u"),
        Instr::I32WrapI64 => format!("{pad}i32.wrap_i64"),
        Instr::I64ReinterpretF64 => format!("{pad}i64.reinterpret_f64"),
        Instr::Select => format!("{pad}select"),

        Instr::RefNull(ht) => format!("{pad}ref.null {}", emit_heap_type(ht)),
        Instr::RefIsNull => format!("{pad}ref.is_null"),
        Instr::RefAsNonNull => format!("{pad}ref.as_non_null"),
        Instr::RefEq => format!("{pad}ref.eq"),
        Instr::RefI31 => format!("{pad}ref.i31"),
        Instr::I31GetS => format!("{pad}i31.get_s"),
        Instr::I31GetU => format!("{pad}i31.get_u"),
        Instr::RefCast { nullable, heap } => {
            let ht = emit_heap_type(heap);
            if *nullable {
                format!("{pad}ref.cast (ref null {ht})")
            } else {
                format!("{pad}ref.cast (ref {ht})")
            }
        }
        Instr::AnyConvertExtern => format!("{pad}any.convert_extern"),
        Instr::ExternConvertAny => format!("{pad}extern.convert_any"),
        Instr::RefTest { nullable, heap } => {
            let ht = emit_heap_type(heap);
            if *nullable {
                format!("{pad}ref.test (ref null {ht})")
            } else {
                format!("{pad}ref.test (ref {ht})")
            }
        }

        Instr::StructNew(ty) => format!("{pad}struct.new ${ty}"),
        Instr::StructGet(ty, idx) => format!("{pad}struct.get ${ty} {idx}"),
        Instr::StructGetS(ty, idx) => format!("{pad}struct.get_s ${ty} {idx}"),
        Instr::StructSet(ty, idx) => format!("{pad}struct.set ${ty} {idx}"),

        Instr::ArrayNew(ty) => format!("{pad}array.new ${ty}"),
        Instr::ArrayNewDefault(ty) => format!("{pad}array.new_default ${ty}"),
        Instr::ArrayNewFixed(ty, n) => format!("{pad}array.new_fixed ${ty} {n}"),
        Instr::ArrayNewData(ty, idx) => format!("{pad}array.new_data ${ty} {idx}"),
        Instr::ArrayGet(ty) => format!("{pad}array.get ${ty}"),
        Instr::ArrayGetU(ty) => format!("{pad}array.get_u ${ty}"),
        Instr::ArraySet(ty) => format!("{pad}array.set ${ty}"),
        Instr::ArrayLen => format!("{pad}array.len"),
        Instr::ArrayCopy(dst, src) => format!("{pad}array.copy ${dst} ${src}"),

        Instr::Call(f) => format!("{pad}call ${f}"),
        Instr::CallIndirect { ty, table } => {
            format!("{pad}call_indirect (type ${ty}) (table {table})")
        }
        Instr::RefFunc(f) => format!("{pad}ref.func ${f}"),
        Instr::CallRef(ty) => format!("{pad}call_ref ${ty}"),
        Instr::ReturnCall(f) => format!("{pad}return_call ${f}"),
        Instr::ReturnCallRef(ty) => format!("{pad}return_call_ref ${ty}"),

        Instr::Drop => format!("{pad}drop"),
        Instr::Return => format!("{pad}return"),
        Instr::Unreachable => format!("{pad}unreachable"),
        Instr::Nop => format!("{pad}nop"),

        Instr::If {
            result,
            then_body,
            else_body,
        } => {
            let mut s = format!("{pad}(if");
            if let Some(r) = result {
                s.push_str(&format!(" (result {})", emit_val_type(r)));
            }
            s.push_str(&format!("\n{pad}  (then"));
            for i in then_body {
                s.push('\n');
                s.push_str(&emit_instr(i, indent + 4));
            }
            s.push(')');
            if !else_body.is_empty() {
                s.push_str(&format!("\n{pad}  (else"));
                for i in else_body {
                    s.push('\n');
                    s.push_str(&emit_instr(i, indent + 4));
                }
                s.push(')');
            }
            s.push(')');
            s
        }

        Instr::Block {
            label,
            result,
            body,
        } => {
            let mut s = format!("{pad}(block ${label}");
            if let Some(r) = result {
                s.push_str(&format!(" (result {})", emit_val_type(r)));
            }
            for i in body {
                s.push('\n');
                s.push_str(&emit_instr(i, indent + 2));
            }
            s.push(')');
            s
        }

        Instr::Loop {
            label,
            result,
            body,
        } => {
            let mut s = format!("{pad}(loop ${label}");
            if let Some(r) = result {
                s.push_str(&format!(" (result {})", emit_val_type(r)));
            }
            for i in body {
                s.push('\n');
                s.push_str(&emit_instr(i, indent + 2));
            }
            s.push(')');
            s
        }

        Instr::Br(label) => format!("{pad}br ${label}"),
        Instr::BrIf(label) => format!("{pad}br_if ${label}"),
        Instr::BrTable { targets, default } => {
            let targets_str: String = targets
                .iter()
                .map(|t| format!("${t}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{pad}br_table {targets_str} ${default}")
        }
    }
}
