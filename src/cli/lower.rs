use anyhow::Result;
use std::fs;

pub fn lower_file(file_path: &str) -> Result<()> {
    let source = fs::read_to_string(file_path)?;

    // Parse
    let (ast, file_registry) = crate::syntax::parse_source(&source, file_path)?;

    // Resolve names
    let (type_env, value_env) = match crate::types::Resolver::resolve(&ast) {
        Ok(envs) => envs,
        Err(errors) => {
            for error in &errors {
                eprintln!("{}", error.format(&file_registry, None));
            }
            anyhow::bail!("Name resolution failed with {} errors", errors.len());
        }
    };

    // Type check — returns TypeMap and TypeEnv
    let (type_map, checked_type_env) =
        match crate::types::TypeChecker::check_module(&ast, type_env.clone(), value_env) {
            Ok(result) => result,
            Err(errors) => {
                for error in &errors {
                    eprintln!("{}", error.format(&file_registry, Some(&type_env)));
                }
                anyhow::bail!("Type checking failed with {} errors", errors.len());
            }
        };

    // Lower to Core IR
    let lowerer = crate::ir::Lowerer::new(type_map, checked_type_env);
    match lowerer.lower_module(&ast) {
        Ok(core_module) => {
            println!("// Core IR for: {}", file_path);
            println!("// {} function(s)", core_module.functions.len());
            if let Some(main_id) = core_module.main_func_id {
                println!("// main = FuncId({})", main_id.0);
            }
            println!();
            for func in &core_module.functions {
                println!("fn {}  [FuncId({})]  params={:?}", func.name, func.func_id.0, func.params);
                println!("  return_ty: {:?}", func.return_ty);
                println!("  body:");
                print_core_expr(&func.body, 4);
                println!();
            }
            Ok(())
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("{}", error.format(&file_registry));
            }
            anyhow::bail!("Lowering failed with {} errors", errors.len());
        }
    }
}

/// Simple recursive pretty-printer for Core IR expressions.
fn print_core_expr(expr: &crate::ir::CoreExpr, indent: usize) {
    use crate::ir::CoreExprKind::*;
    let pad = " ".repeat(indent);
    match &expr.kind {
        LitInt(n) => println!("{}LitInt({}) : {:?}", pad, n, expr.ty),
        LitFloat(f) => println!("{}LitFloat({}) : {:?}", pad, f, expr.ty),
        LitBool(b) => println!("{}LitBool({}) : {:?}", pad, b, expr.ty),
        LitStr(s) => println!("{}LitStr({:?}) : {:?}", pad, s, expr.ty),
        LitVoid => println!("{}LitVoid", pad),
        Local(id) => println!("{}Local({}) : {:?}", pad, id.0, expr.ty),
        GlobalFunc(id) => println!("{}GlobalFunc({}) : {:?}", pad, id.0, expr.ty),
        Let { local, value, body } => {
            println!("{}Let L{} =", pad, local.0);
            print_core_expr(value, indent + 2);
            println!("{}in", pad);
            print_core_expr(body, indent + 2);
        }
        Assign { local, value } => {
            println!("{}Assign L{} =", pad, local.0);
            print_core_expr(value, indent + 2);
        }
        BinOp { op, left, right } => {
            println!("{}BinOp({:?}) : {:?}", pad, op, expr.ty);
            print_core_expr(left, indent + 2);
            print_core_expr(right, indent + 2);
        }
        UnOp { op, expr: inner } => {
            println!("{}UnOp({:?}) : {:?}", pad, op, expr.ty);
            print_core_expr(inner, indent + 2);
        }
        Call { callee, args } => {
            println!("{}Call : {:?}", pad, expr.ty);
            print_core_expr(callee, indent + 2);
            for arg in args {
                print_core_expr(arg, indent + 4);
            }
        }
        Lambda { params, body } => {
            println!("{}Lambda params={:?}", pad, params);
            print_core_expr(body, indent + 2);
        }
        If { cond, then_branch, else_branch } => {
            println!("{}If : {:?}", pad, expr.ty);
            println!("{}  cond:", pad);
            print_core_expr(cond, indent + 4);
            println!("{}  then:", pad);
            print_core_expr(then_branch, indent + 4);
            println!("{}  else:", pad);
            print_core_expr(else_branch, indent + 4);
        }
        Match { scrutinee, arms } => {
            println!("{}Match : {:?}", pad, expr.ty);
            print_core_expr(scrutinee, indent + 2);
            for arm in arms {
                println!("{}  arm {:?} =>", pad, arm.pattern);
                print_core_expr(&arm.body, indent + 4);
            }
        }
        Loop { body } => {
            println!("{}Loop : {:?}", pad, expr.ty);
            print_core_expr(body, indent + 2);
        }
        Break { value: None } => println!("{}Break", pad),
        Break { value: Some(v) } => {
            println!("{}Break :", pad);
            print_core_expr(v, indent + 2);
        }
        Continue => println!("{}Continue", pad),
        Return { value: None } => println!("{}Return void", pad),
        Return { value: Some(v) } => {
            println!("{}Return:", pad);
            print_core_expr(v, indent + 2);
        }
        Record { type_id, fields } => {
            println!("{}Record(Type#{}) : {:?}", pad, type_id.0, expr.ty);
            for (fid, val) in fields {
                println!("{}  field {}:", pad, fid.0);
                print_core_expr(val, indent + 4);
            }
        }
        RecordGet { target, field } => {
            println!("{}RecordGet .{} : {:?}", pad, field.0, expr.ty);
            print_core_expr(target, indent + 2);
        }
        Variant { type_id, variant, args } => {
            println!("{}Variant(Type#{} .{}) : {:?}", pad, type_id.0, variant.0, expr.ty);
            for arg in args {
                print_core_expr(arg, indent + 2);
            }
        }
        ArrayLit { elements } => {
            println!("{}ArrayLit[{}] : {:?}", pad, elements.len(), expr.ty);
            for e in elements {
                print_core_expr(e, indent + 2);
            }
        }
        Index { base, index } => {
            println!("{}Index : {:?}", pad, expr.ty);
            print_core_expr(base, indent + 2);
            print_core_expr(index, indent + 2);
        }
    }
}
