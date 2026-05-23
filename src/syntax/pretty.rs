// Pretty-printing for AST structures
use crate::syntax::ast::*;
use std::fmt::Write;

pub fn print_source_file(file: &SourceFile) -> String {
    let mut output = String::new();
    writeln!(&mut output, "SourceFile:").unwrap();
    for (i, item) in file.items.iter().enumerate() {
        writeln!(&mut output, "  Item #{}:", i + 1).unwrap();
        print_item(item, &mut output, 2);
    }
    output
}

fn print_item(item: &Item, out: &mut String, indent: usize) {
    let prefix = " ".repeat(indent);
    match item {
        Item::Import(decl) => {
            let path = if decl.is_stdlib {
                format!("@{}", decl.module_path.join("."))
            } else if decl.is_relative {
                format!(".{}", decl.module_path.join("."))
            } else {
                decl.module_path.join(".")
            };
            let alias_str = decl
                .alias
                .as_deref()
                .map(|a| format!(" as {}", a))
                .unwrap_or_default();
            writeln!(out, "{}Import: {}{}", prefix, path, alias_str).unwrap();
        }
        Item::TypeDecl(decl) => {
            write!(out, "{}TypeDecl", prefix).unwrap();
            if decl.is_pub {
                write!(out, " (pub)").unwrap();
            }
            write!(out, ": {}", decl.name).unwrap();
            if !decl.type_params.is_empty() {
                let params = decl
                    .type_params
                    .iter()
                    .map(|p| {
                        if p.bounds.is_empty() {
                            p.name.clone()
                        } else {
                            format!("{}: {}", p.name, p.bounds.join(" + "))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(out, "<{}>", params).unwrap();
            }
            writeln!(out).unwrap();
            match &decl.definition {
                TypeDef::Record { fields } => {
                    writeln!(out, "{}  Record {{", prefix).unwrap();
                    for field in fields {
                        print_indent(out, indent + 4);
                        write!(out, "{}: ", field.name).unwrap();
                        print_type(&field.ty, out);
                        writeln!(out).unwrap();
                    }
                    writeln!(out, "{}  }}", prefix).unwrap();
                }
                TypeDef::Sum { variants } => {
                    writeln!(out, "{}  Sum {{", prefix).unwrap();
                    for variant in variants {
                        print_indent(out, indent + 4);
                        write!(out, "{}", variant.name).unwrap();
                        if !variant.fields.is_empty() {
                            write!(out, "(").unwrap();
                            for (i, ty) in variant.fields.iter().enumerate() {
                                if i > 0 {
                                    write!(out, ", ").unwrap();
                                }
                                print_type(ty, out);
                            }
                            write!(out, ")").unwrap();
                        }
                        writeln!(out).unwrap();
                    }
                    writeln!(out, "{}  }}", prefix).unwrap();
                }
                TypeDef::Alias { ty } => {
                    write!(out, "{}  Alias: ", prefix).unwrap();
                    print_type(ty, out);
                    writeln!(out).unwrap();
                }
            }
        }
        Item::Function(decl) => {
            write!(out, "{}Function", prefix).unwrap();
            if decl.is_pub {
                write!(out, " (pub)").unwrap();
            }
            write!(out, ": {}", decl.name).unwrap();
            if !decl.type_params.is_empty() {
                let params = decl
                    .type_params
                    .iter()
                    .map(|p| {
                        if p.bounds.is_empty() {
                            p.name.clone()
                        } else {
                            format!("{}: {}", p.name, p.bounds.join(" + "))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(out, "<{}>", params).unwrap();
            }
            write!(out, "(").unwrap();
            for (i, param) in decl.params.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{}", param.name).unwrap();
                if let Some(ty) = &param.ty {
                    write!(out, ": ").unwrap();
                    print_type(ty, out);
                }
            }
            write!(out, ")").unwrap();
            if let Some(ret) = &decl.return_type {
                write!(out, " -> ").unwrap();
                print_type(ret, out);
            }
            writeln!(out).unwrap();
            print_block(&decl.body, out, indent + 2);
        }
        Item::ExternFunction(decl) => {
            write!(out, "{}ExternFunction", prefix).unwrap();
            if decl.is_pub {
                write!(out, " (pub)").unwrap();
            }
            write!(out, " [module={}]", decl.module).unwrap();
            write!(out, ": {}", decl.name).unwrap();
            write!(out, "(").unwrap();
            for (i, param) in decl.params.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{}", param.name).unwrap();
                if let Some(ty) = &param.ty {
                    write!(out, ": ").unwrap();
                    print_type(ty, out);
                }
            }
            write!(out, ")").unwrap();
            if let Some(ret) = &decl.return_type {
                write!(out, " -> ").unwrap();
                print_type(ret, out);
            }
            writeln!(out).unwrap();
        }
        Item::ExternType(decl) => {
            write!(out, "{}ExternType", prefix).unwrap();
            if decl.is_pub {
                write!(out, " (pub)").unwrap();
            }
            writeln!(out, " [module={}]: {}", decl.module, decl.name).unwrap();
        }
        Item::Stmt(stmt) => {
            print_stmt(stmt, out, indent);
        }
    }
}

fn print_block(block: &Block, out: &mut String, indent: usize) {
    let prefix = " ".repeat(indent);
    writeln!(out, "{}Block:", prefix).unwrap();
    for stmt in &block.stmts {
        print_stmt(stmt, out, indent + 2);
    }
}

fn print_stmt(stmt: &Stmt, out: &mut String, indent: usize) {
    let prefix = " ".repeat(indent);
    match stmt {
        Stmt::Let {
            pattern, ty, value, ..
        } => {
            write!(out, "{}Let ", prefix).unwrap();
            print_pattern(pattern, out);
            if let Some(ty) = ty {
                write!(out, ": ").unwrap();
                print_type(ty, out);
            }
            writeln!(out, " =").unwrap();
            print_expr(value, out, indent + 2);
        }
        Stmt::For {
            pattern,
            index_pattern,
            iter,
            body,
            ..
        } => {
            write!(out, "{}For ", prefix).unwrap();
            print_pattern(pattern, out);
            if let Some(idx) = index_pattern {
                write!(out, ", ").unwrap();
                print_pattern(idx, out);
            }
            writeln!(out, " in").unwrap();
            print_expr(iter, out, indent + 2);
            print_block(body, out, indent + 2);
        }
        Stmt::ForCond { cond, body, .. } => {
            writeln!(out, "{}For (cond)", prefix).unwrap();
            print_expr(cond, out, indent + 2);
            print_block(body, out, indent + 2);
        }
        Stmt::Break { value, .. } => {
            write!(out, "{}Break", prefix).unwrap();
            if let Some(val) = value {
                writeln!(out).unwrap();
                print_expr(val, out, indent + 2);
            } else {
                writeln!(out).unwrap();
            }
        }
        Stmt::Continue { .. } => {
            writeln!(out, "{}Continue", prefix).unwrap();
        }
        Stmt::Return { value, .. } => {
            write!(out, "{}Return", prefix).unwrap();
            if let Some(val) = value {
                writeln!(out).unwrap();
                print_expr(val, out, indent + 2);
            } else {
                writeln!(out).unwrap();
            }
        }
        Stmt::Expr(expr) => {
            print_expr(expr, out, indent);
        }
        Stmt::Defer { expr, .. } => {
            writeln!(out, "{}Defer", prefix).unwrap();
            print_expr(expr, out, indent + 2);
        }
    }
}

fn print_expr(expr: &Expr, out: &mut String, indent: usize) {
    let prefix = " ".repeat(indent);
    match &expr.kind {
        ExprKind::Literal(lit) => {
            write!(out, "{}", prefix).unwrap();
            print_literal(lit, out);
            writeln!(out).unwrap();
        }
        ExprKind::Ident(name) => {
            writeln!(out, "{}{}", prefix, name).unwrap();
        }
        ExprKind::Binary { op, left, right } => {
            writeln!(out, "{}({:?})", prefix, op).unwrap();
            print_expr(left, out, indent + 2);
            print_expr(right, out, indent + 2);
        }
        ExprKind::Unary { op, expr } => {
            writeln!(out, "{}({:?})", prefix, op).unwrap();
            print_expr(expr, out, indent + 2);
        }
        ExprKind::Call { callee, args } => {
            writeln!(out, "{}Call", prefix).unwrap();
            print_expr(callee, out, indent + 2);
            for arg in args {
                print_expr(arg, out, indent + 2);
            }
        }
        ExprKind::FieldAccess { base, field } => {
            writeln!(out, "{}FieldAccess: .{}", prefix, field).unwrap();
            print_expr(base, out, indent + 2);
        }
        ExprKind::Index { base, index } => {
            writeln!(out, "{}Index", prefix).unwrap();
            print_expr(base, out, indent + 2);
            print_expr(index, out, indent + 2);
        }
        ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            writeln!(out, "{}If", prefix).unwrap();
            writeln!(out, "{}  condition:", prefix).unwrap();
            print_expr(cond, out, indent + 4);
            writeln!(out, "{}  then:", prefix).unwrap();
            print_expr(then_branch, out, indent + 4);
            if let Some(else_br) = else_branch {
                writeln!(out, "{}  else:", prefix).unwrap();
                print_expr(else_br, out, indent + 4);
            }
        }
        ExprKind::Case { scrutinee, arms } => {
            writeln!(out, "{}Case", prefix).unwrap();
            print_expr(scrutinee, out, indent + 2);
            for arm in arms {
                print_indent(out, indent + 2);
                print_pattern(&arm.pattern, out);
                writeln!(out, " =>").unwrap();
                print_expr(&arm.body, out, indent + 4);
            }
        }
        ExprKind::Cond { arms } => {
            writeln!(out, "{}Cond", prefix).unwrap();
            for arm in arms {
                if let Some(cond) = &arm.condition {
                    writeln!(out, "{}  condition:", prefix).unwrap();
                    print_expr(cond, out, indent + 4);
                } else {
                    writeln!(out, "{}  _ =>", prefix).unwrap();
                }
                writeln!(out, "{}  body:", prefix).unwrap();
                print_expr(&arm.body, out, indent + 4);
            }
        }
        ExprKind::Array { elements } => {
            writeln!(out, "{}Array[{}]", prefix, elements.len()).unwrap();
            for expr in elements {
                print_expr(expr, out, indent + 2);
            }
        }
        ExprKind::RecordLit { name, fields } => {
            if let Some(name) = name {
                writeln!(out, "{}RecordLit: {} {{", prefix, name).unwrap();
            } else {
                writeln!(out, "{}RecordLit {{", prefix).unwrap();
            }
            for (field_name, value) in fields {
                print_indent(out, indent + 2);
                writeln!(out, "{} =", field_name).unwrap();
                print_expr(value, out, indent + 4);
            }
            writeln!(out, "{}}}", prefix).unwrap();
        }
        ExprKind::VariantLit { name, fields } => {
            write!(out, "{}VariantLit: .{}", prefix, name).unwrap();
            if !fields.is_empty() {
                writeln!(out).unwrap();
                for field in fields {
                    print_expr(field, out, indent + 2);
                }
            } else {
                writeln!(out).unwrap();
            }
        }
        ExprKind::Block(block) => {
            print_block(block, out, indent);
        }
        ExprKind::Function(func) => {
            write!(out, "{}Function(", prefix).unwrap();
            for (i, param) in func.params.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{}", param.name).unwrap();
                if let Some(ty) = &param.ty {
                    write!(out, ": ").unwrap();
                    print_type(ty, out);
                }
            }
            write!(out, ")").unwrap();
            if let Some(ret) = &func.return_type {
                write!(out, " -> ").unwrap();
                print_type(ret, out);
            }
            writeln!(out).unwrap();
            print_expr(&func.body, out, indent + 2);
        }
        ExprKind::Try { expr } => {
            writeln!(out, "{}Try", prefix).unwrap();
            print_expr(expr, out, indent + 2);
        }
        ExprKind::Collect {
            pattern,
            index_pattern,
            iter,
            body,
        } => {
            write!(out, "{}Collect ", prefix).unwrap();
            print_pattern(pattern, out);
            if let Some(idx_pat) = index_pattern {
                write!(out, ", ").unwrap();
                print_pattern(idx_pat, out);
            }
            writeln!(out, " in").unwrap();
            print_expr(iter, out, indent + 2);
            print_expr(body, out, indent + 2);
        }
        ExprKind::CollectWhile { cond, body } => {
            writeln!(out, "{}CollectWhile", prefix).unwrap();
            print_expr(cond, out, indent + 2);
            print_expr(body, out, indent + 2);
        }
        ExprKind::StringInterpolation { parts } => {
            writeln!(out, "{}StringInterpolation[{}]", prefix, parts.len()).unwrap();
            for part in parts {
                match part {
                    StringPart::Literal(s) => {
                        print_indent(out, indent + 2);
                        writeln!(out, "\"{}\"", s).unwrap();
                    }
                    StringPart::Interpolation(expr) => {
                        print_indent(out, indent + 2);
                        writeln!(out, "${{...}}").unwrap();
                        print_expr(expr, out, indent + 4);
                    }
                }
            }
        }
    }
}

fn print_type(ty: &Type, out: &mut String) {
    match ty {
        Type::Named { name, args, .. } => {
            write!(out, "{}", name).unwrap();
            if !args.is_empty() {
                write!(out, "<").unwrap();
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(out, ", ").unwrap();
                    }
                    print_type(arg, out);
                }
                write!(out, ">").unwrap();
            }
        }
        Type::Record { fields, .. } => {
            write!(out, ".{{ ").unwrap();
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{}: ", field.name).unwrap();
                print_type(&field.ty, out);
            }
            write!(out, " }}").unwrap();
        }
        Type::Function { params, ret, .. } => {
            write!(out, "fn(").unwrap();
            for (i, param) in params.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ").unwrap();
                }
                print_type(param, out);
            }
            write!(out, ") ").unwrap();
            print_type(ret, out);
        }
    }
}

fn print_pattern(pattern: &Pattern, out: &mut String) {
    match pattern {
        Pattern::Wildcard(_) => write!(out, "_").unwrap(),
        Pattern::Ident(name, _) => write!(out, "{}", name).unwrap(),
        Pattern::Literal(lit, _) => print_literal(lit, out),
        Pattern::Variant {
            type_name,
            name,
            fields,
            ..
        } => {
            match type_name {
                Some(tname) => write!(out, "{}.{}", tname, name).unwrap(),
                None => write!(out, ".{}", name).unwrap(),
            };
            if !fields.is_empty() {
                write!(out, "(").unwrap();
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(out, ", ").unwrap();
                    }
                    print_pattern(field, out);
                }
                write!(out, ")").unwrap();
            }
        }
    }
}

fn print_literal(lit: &Literal, out: &mut String) {
    match lit {
        Literal::Int(n) => write!(out, "{}", n).unwrap(),
        Literal::Float(f) => write!(out, "{}", f).unwrap(),
        Literal::String(s) => write!(out, "\"{}\"", s).unwrap(),
        Literal::Bool(b) => write!(out, "{}", b).unwrap(),
    }
}

fn print_indent(out: &mut String, indent: usize) {
    write!(out, "{}", " ".repeat(indent)).unwrap();
}
