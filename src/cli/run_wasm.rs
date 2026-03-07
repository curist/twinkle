use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use wasmtime::{
    AnyRef, ArrayRef, ArrayRefPre, ArrayType, AsContext, AsContextMut, Caller, Config, Engine,
    ExternType, FuncType, HeapType, Linker, Module, Rooted, Store, Val, ValType,
};

#[derive(Default)]
struct HostOutput {
    stdout: String,
    stderr: String,
    cwd: PathBuf,
    argv: Vec<String>,
    env: HashMap<String, String>,
    exit_code: Option<i64>,
}

#[derive(Default)]
struct HostImportTypes {
    print: Option<FuncType>,
    println: Option<FuncType>,
    error: Option<FuncType>,
    eprint: Option<FuncType>,
    eprintln: Option<FuncType>,
    f64_to_string: Option<FuncType>,
    read_file: Option<FuncType>,
    write_file: Option<FuncType>,
    write_bytes: Option<FuncType>,
    mkdirp: Option<FuncType>,
    list_dir: Option<FuncType>,
    exists: Option<FuncType>,
    args: Option<FuncType>,
    env: Option<FuncType>,
    cwd: Option<FuncType>,
    exit: Option<FuncType>,
    parse_int: Option<FuncType>,
    parse_float: Option<FuncType>,
    string_array_ty: Option<ArrayType>,
    runtime_array_ty: Option<ArrayType>,
}

impl HostImportTypes {
    fn from_module(module: &Module) -> Result<Self> {
        let mut out = Self::default();

        for import in module.imports() {
            if import.module() != "host" {
                continue;
            }

            let name = import.name();
            let func_ty = match import.ty() {
                ExternType::Func(f) => f,
                other => bail!("unsupported non-function host import {name}: {other:?}"),
            };

            match name {
                "print" | "println" | "error" | "eprint" | "eprintln" => {
                    let candidate = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, candidate)?;
                    let slot = match name {
                        "print" => &mut out.print,
                        "println" => &mut out.println,
                        "error" => &mut out.error,
                        "eprint" => &mut out.eprint,
                        "eprintln" => &mut out.eprintln,
                        _ => unreachable!(),
                    };
                    ensure!(slot.is_none(), "duplicate host import: {name}");
                    *slot = Some(func_ty);
                }
                "f64_to_string" => {
                    let candidate = concrete_array_from_func_result(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, candidate)?;
                    ensure!(
                        out.f64_to_string.is_none(),
                        "duplicate host import: f64_to_string"
                    );
                    out.f64_to_string = Some(func_ty);
                }
                "read_file" => {
                    let path_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    let result_ty = concrete_array_from_func_result(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, path_ty)?;
                    merge_string_array_ty(&mut out.string_array_ty, result_ty)?;
                    ensure!(out.read_file.is_none(), "duplicate host import: read_file");
                    out.read_file = Some(func_ty);
                }
                "write_file" => {
                    let path_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    let content_ty = concrete_array_from_func_param(&func_ty, 1)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, path_ty)?;
                    merge_string_array_ty(&mut out.string_array_ty, content_ty)?;
                    ensure!(
                        out.write_file.is_none(),
                        "duplicate host import: write_file"
                    );
                    out.write_file = Some(func_ty);
                }
                "write_bytes" => {
                    let path_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    let bytes_ty = concrete_array_from_func_param(&func_ty, 1)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, path_ty)?;
                    merge_runtime_array_ty(&mut out.runtime_array_ty, bytes_ty)?;
                    ensure!(
                        out.write_bytes.is_none(),
                        "duplicate host import: write_bytes"
                    );
                    out.write_bytes = Some(func_ty);
                }
                "mkdirp" => {
                    let path_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, path_ty)?;
                    ensure!(out.mkdirp.is_none(), "duplicate host import: mkdirp");
                    out.mkdirp = Some(func_ty);
                }
                "list_dir" => {
                    let path_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    let result_ty = concrete_array_from_func_result(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, path_ty)?;
                    merge_runtime_array_ty(&mut out.runtime_array_ty, result_ty)?;
                    ensure!(out.list_dir.is_none(), "duplicate host import: list_dir");
                    out.list_dir = Some(func_ty);
                }
                "exists" => {
                    let path_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, path_ty)?;
                    ensure!(out.exists.is_none(), "duplicate host import: exists");
                    out.exists = Some(func_ty);
                }
                "args" => {
                    let result_ty = concrete_array_from_func_result(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_runtime_array_ty(&mut out.runtime_array_ty, result_ty)?;
                    ensure!(out.args.is_none(), "duplicate host import: args");
                    out.args = Some(func_ty);
                }
                "env" => {
                    let key_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    let result_ty = concrete_array_from_func_result(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, key_ty)?;
                    merge_runtime_array_ty(&mut out.runtime_array_ty, result_ty)?;
                    ensure!(out.env.is_none(), "duplicate host import: env");
                    out.env = Some(func_ty);
                }
                "cwd" => {
                    let result_ty = concrete_array_from_func_result(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, result_ty)?;
                    ensure!(out.cwd.is_none(), "duplicate host import: cwd");
                    out.cwd = Some(func_ty);
                }
                "exit" => {
                    ensure!(out.exit.is_none(), "duplicate host import: exit");
                    out.exit = Some(func_ty);
                }
                "parse_int" => {
                    let str_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, str_ty)?;
                    ensure!(out.parse_int.is_none(), "duplicate host import: parse_int");
                    out.parse_int = Some(func_ty);
                }
                "parse_float" => {
                    let str_ty = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, str_ty)?;
                    ensure!(
                        out.parse_float.is_none(),
                        "duplicate host import: parse_float"
                    );
                    out.parse_float = Some(func_ty);
                }
                "from_char_code" => {
                    // Accepted but no-op: from_char_code is handled inline in codegen
                    // for ASCII range. Future: full Unicode support via host.
                }
                other => bail!("unsupported host import: host.{other}"),
            }
        }

        Ok(out)
    }

    fn define_all(&self, linker: &mut Linker<HostOutput>) -> Result<()> {
        if let Some(ty) = &self.print {
            linker.func_new(
                "host",
                "print",
                ty.clone(),
                |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.print expected 1 argument");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    caller.data_mut().stdout.push_str(&text);
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.println {
            linker.func_new(
                "host",
                "println",
                ty.clone(),
                |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.println expected 1 argument");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    caller.data_mut().stdout.push_str(&text);
                    caller.data_mut().stdout.push('\n');
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.error {
            linker.func_new(
                "host",
                "error",
                ty.clone(),
                |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.error expected 1 argument");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    caller.data_mut().stderr.push_str(&text);
                    caller.data_mut().stderr.push('\n');
                    Err(anyhow!("host.error: {text}"))
                },
            )?;
        }

        if let Some(ty) = &self.eprint {
            linker.func_new(
                "host",
                "eprint",
                ty.clone(),
                |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.eprint expected 1 argument");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    caller.data_mut().stderr.push_str(&text);
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.eprintln {
            linker.func_new(
                "host",
                "eprintln",
                ty.clone(),
                |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.eprintln expected 1 argument");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    caller.data_mut().stderr.push_str(&text);
                    caller.data_mut().stderr.push('\n');
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.f64_to_string {
            let string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.f64_to_string"))?;
            linker.func_new(
                "host",
                "f64_to_string",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.f64_to_string expected 1 argument");
                    ensure!(results.len() == 1, "host.f64_to_string expected 1 result");

                    let n = params[0].f64().ok_or_else(|| {
                        anyhow!(
                            "host.f64_to_string expected f64 argument, got {:?}",
                            params[0].ty(caller.as_context())
                        )
                    })?;
                    let rendered = n.to_string();
                    let rendered_ref =
                        encode_runtime_string(&mut caller, &string_array_ty, &rendered)?;
                    results[0] = Val::AnyRef(Some(rendered_ref));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.read_file {
            let string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.read_file"))?;
            linker.func_new(
                "host",
                "read_file",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.read_file expected 1 argument");
                    ensure!(results.len() == 1, "host.read_file expected 1 result");
                    let logical_path = decode_runtime_string(&mut caller, &params[0])?;
                    let host_path = resolve_host_path(caller.data(), &logical_path);
                    let content = std::fs::read_to_string(&host_path).with_context(|| {
                        format!("host.read_file failed for '{}'", host_path.display())
                    })?;
                    let content_ref =
                        encode_runtime_string(&mut caller, &string_array_ty, &content)?;
                    results[0] = Val::AnyRef(Some(content_ref));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.write_file {
            linker.func_new(
                "host",
                "write_file",
                ty.clone(),
                move |mut caller, params, _results| {
                    ensure!(params.len() == 2, "host.write_file expected 2 arguments");
                    let logical_path = decode_runtime_string(&mut caller, &params[0])?;
                    let content = decode_runtime_string(&mut caller, &params[1])?;
                    let host_path = resolve_host_path(caller.data(), &logical_path);
                    std::fs::write(&host_path, content).with_context(|| {
                        format!("host.write_file failed for '{}'", host_path.display())
                    })?;
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.write_bytes {
            linker.func_new(
                "host",
                "write_bytes",
                ty.clone(),
                move |mut caller, params, _results| {
                    ensure!(params.len() == 2, "host.write_bytes expected 2 arguments");
                    let logical_path = decode_runtime_string(&mut caller, &params[0])?;
                    let bytes = decode_runtime_bytes(&mut caller, &params[1])?;
                    let host_path = resolve_host_path(caller.data(), &logical_path);
                    std::fs::write(&host_path, bytes).with_context(|| {
                        format!("host.write_bytes failed for '{}'", host_path.display())
                    })?;
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.mkdirp {
            linker.func_new(
                "host",
                "mkdirp",
                ty.clone(),
                move |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.mkdirp expected 1 argument");
                    let logical_path = decode_runtime_string(&mut caller, &params[0])?;
                    let host_path = resolve_host_path(caller.data(), &logical_path);
                    std::fs::create_dir_all(&host_path).with_context(|| {
                        format!("host.mkdirp failed for '{}'", host_path.display())
                    })?;
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.list_dir {
            let string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.list_dir"))?;
            let runtime_array_ty = self
                .runtime_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing runtime array type for host.list_dir"))?;
            linker.func_new(
                "host",
                "list_dir",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.list_dir expected 1 argument");
                    ensure!(results.len() == 1, "host.list_dir expected 1 result");
                    let logical_path = decode_runtime_string(&mut caller, &params[0])?;
                    let host_path = resolve_host_path(caller.data(), &logical_path);
                    let mut names = std::fs::read_dir(&host_path)
                        .with_context(|| {
                            format!("host.list_dir failed for '{}'", host_path.display())
                        })?
                        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
                        .collect::<std::io::Result<Vec<_>>>()
                        .with_context(|| {
                            format!("host.list_dir failed for '{}'", host_path.display())
                        })?;
                    names.sort();

                    let mut elems = Vec::with_capacity(names.len());
                    for name in names {
                        let name_ref = encode_runtime_string(&mut caller, &string_array_ty, &name)?;
                        elems.push(Val::AnyRef(Some(name_ref)));
                    }

                    let allocator =
                        ArrayRefPre::new(caller.as_context_mut(), runtime_array_ty.clone());
                    let result = ArrayRef::new_fixed(caller.as_context_mut(), &allocator, &elems)?;
                    results[0] = Val::AnyRef(Some(result.to_anyref()));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.exists {
            linker.func_new(
                "host",
                "exists",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.exists expected 1 argument");
                    ensure!(results.len() == 1, "host.exists expected 1 result");
                    let logical_path = decode_runtime_string(&mut caller, &params[0])?;
                    let host_path = resolve_host_path(caller.data(), &logical_path);
                    results[0] = Val::I32(i32::from(host_path.exists()));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.args {
            let string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.args"))?;
            let runtime_array_ty = self
                .runtime_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing runtime array type for host.args"))?;
            linker.func_new(
                "host",
                "args",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.is_empty(), "host.args expected 0 arguments");
                    ensure!(results.len() == 1, "host.args expected 1 result");
                    let argv = caller.data().argv.clone();
                    let result = encode_runtime_string_array(
                        &mut caller,
                        &string_array_ty,
                        &runtime_array_ty,
                        &argv,
                    )?;
                    results[0] = Val::AnyRef(Some(result));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.env {
            let string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.env"))?;
            let runtime_array_ty = self
                .runtime_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing runtime array type for host.env"))?;
            linker.func_new(
                "host",
                "env",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.env expected 1 argument");
                    ensure!(results.len() == 1, "host.env expected 1 result");
                    let name = decode_runtime_string(&mut caller, &params[0])?;
                    let values = caller
                        .data()
                        .env
                        .get(&name)
                        .cloned()
                        .map(|v| vec![v])
                        .unwrap_or_default();
                    let result = encode_runtime_string_array(
                        &mut caller,
                        &string_array_ty,
                        &runtime_array_ty,
                        &values,
                    )?;
                    results[0] = Val::AnyRef(Some(result));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.cwd {
            let string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.cwd"))?;
            linker.func_new(
                "host",
                "cwd",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.is_empty(), "host.cwd expected 0 arguments");
                    ensure!(results.len() == 1, "host.cwd expected 1 result");
                    let logical = host_path_to_logical(caller.data().cwd.as_path());
                    let cwd_ref = encode_runtime_string(&mut caller, &string_array_ty, &logical)?;
                    results[0] = Val::AnyRef(Some(cwd_ref));
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.exit {
            linker.func_new(
                "host",
                "exit",
                ty.clone(),
                move |mut caller, params, _results| {
                    ensure!(params.len() == 1, "host.exit expected 1 argument");
                    let code = host_exit_code_from_val(&params[0])?;
                    caller.data_mut().exit_code = Some(code);
                    Err(anyhow!("host.exit({code})"))
                },
            )?;
        }

        if let Some(ty) = &self.parse_int {
            let _string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.parse_int"))?;
            linker.func_new(
                "host",
                "parse_int",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.parse_int expected 1 argument");
                    ensure!(results.len() == 2, "host.parse_int expected 2 results");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    match text.parse::<i64>() {
                        Ok(n) => {
                            results[0] = Val::I64(n);
                            results[1] = Val::I32(1);
                        }
                        Err(_) => {
                            results[0] = Val::I64(0);
                            results[1] = Val::I32(0);
                        }
                    }
                    Ok(())
                },
            )?;
        }

        if let Some(ty) = &self.parse_float {
            let _string_array_ty = self
                .string_array_ty
                .clone()
                .ok_or_else(|| anyhow!("missing string array type for host.parse_float"))?;
            linker.func_new(
                "host",
                "parse_float",
                ty.clone(),
                move |mut caller, params, results| {
                    ensure!(params.len() == 1, "host.parse_float expected 1 argument");
                    ensure!(results.len() == 2, "host.parse_float expected 2 results");
                    let text = decode_runtime_string(&mut caller, &params[0])?;
                    match text.parse::<f64>() {
                        Ok(f) => {
                            results[0] = Val::F64(f.to_bits());
                            results[1] = Val::I32(1);
                        }
                        Err(_) => {
                            results[0] = Val::F64(0.0f64.to_bits());
                            results[1] = Val::I32(0);
                        }
                    }
                    Ok(())
                },
            )?;
        }

        Ok(())
    }
}

fn concrete_array_from_func_param(func_ty: &FuncType, index: usize) -> Result<ArrayType> {
    let params = func_ty.params().collect::<Vec<_>>();
    let ty = params
        .get(index)
        .ok_or_else(|| anyhow!("expected at least {} parameter(s)", index + 1))?;
    concrete_array_from_val_type(ty)
}

fn concrete_array_from_func_result(func_ty: &FuncType, index: usize) -> Result<ArrayType> {
    let results = func_ty.results().collect::<Vec<_>>();
    let ty = results
        .get(index)
        .ok_or_else(|| anyhow!("expected at least {} result(s)", index + 1))?;
    concrete_array_from_val_type(ty)
}

fn concrete_array_from_val_type(ty: &ValType) -> Result<ArrayType> {
    let r = match ty {
        ValType::Ref(r) => r,
        other => bail!("expected reference type, got {other}"),
    };
    let array_ty = match r.heap_type() {
        HeapType::ConcreteArray(array_ty) => array_ty.clone(),
        other => bail!("expected concrete array reference type, got (ref {other})"),
    };
    Ok(array_ty)
}

fn merge_string_array_ty(slot: &mut Option<ArrayType>, candidate: ArrayType) -> Result<()> {
    match slot {
        Some(existing) => {
            ensure!(
                ArrayType::eq(existing, &candidate),
                "host import string types do not match"
            );
        }
        None => {
            *slot = Some(candidate);
        }
    }
    Ok(())
}

fn merge_runtime_array_ty(slot: &mut Option<ArrayType>, candidate: ArrayType) -> Result<()> {
    match slot {
        Some(existing) => {
            ensure!(
                ArrayType::eq(existing, &candidate),
                "host import array types do not match"
            );
        }
        None => {
            *slot = Some(candidate);
        }
    }
    Ok(())
}

fn resolve_host_path(host: &HostOutput, logical_path: &str) -> PathBuf {
    let normalized = if std::path::MAIN_SEPARATOR == '/' {
        logical_path.to_string()
    } else {
        let sep = std::path::MAIN_SEPARATOR.to_string();
        logical_path.replace('/', &sep)
    };
    let path = PathBuf::from(normalized);
    if path.is_absolute() {
        path
    } else {
        host.cwd.join(path)
    }
}

fn host_path_to_logical(path: &Path) -> String {
    let s = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '/' {
        s.into_owned()
    } else {
        s.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

fn decode_runtime_string(caller: &mut Caller<'_, HostOutput>, val: &Val) -> Result<String> {
    let anyref = val
        .anyref()
        .ok_or_else(|| anyhow!("expected anyref argument for host string"))?;

    let Some(anyref) = anyref else {
        return Ok(String::new());
    };

    let array = anyref
        .as_array(caller.as_context())?
        .ok_or_else(|| anyhow!("expected arrayref argument for host string"))?;

    let mut bytes = Vec::new();
    for elem in array.elems(caller.as_context_mut())? {
        let byte = match elem {
            Val::I32(i) if (0..=255).contains(&i) => i as u8,
            Val::I32(i) => bail!("runtime string byte out of range: {i}"),
            _other => bail!("expected i32 byte element in runtime string"),
        };
        bytes.push(byte);
    }

    decode_runtime_utf8_bytes(bytes)
}

fn decode_runtime_bytes(caller: &mut Caller<'_, HostOutput>, val: &Val) -> Result<Vec<u8>> {
    let anyref = val
        .anyref()
        .ok_or_else(|| anyhow!("expected anyref argument for host byte array"))?;

    let Some(anyref) = anyref else {
        return Ok(Vec::new());
    };

    let array = anyref
        .as_array(caller.as_context())?
        .ok_or_else(|| anyhow!("expected arrayref argument for host byte array"))?;

    let elems = array.elems(caller.as_context_mut())?.collect::<Vec<_>>();

    let mut bytes = Vec::new();
    for elem in elems {
        bytes.push(decode_runtime_byte_elem(caller, &elem)?);
    }
    Ok(bytes)
}

fn decode_runtime_byte_elem(caller: &mut Caller<'_, HostOutput>, elem: &Val) -> Result<u8> {
    match elem {
        Val::I32(i) => i64_to_u8(i64::from(*i)),
        Val::I64(i) => i64_to_u8(*i),
        Val::AnyRef(Some(anyref)) => {
            if let Some(i31) = anyref.as_i31(caller.as_context())? {
                return i64_to_u8(i64::from(i31.get_i32()));
            }
            let s = anyref
                .as_struct(caller.as_context())?
                .ok_or_else(|| anyhow!("expected BoxedInt struct in host byte array"))?;
            let field0 = s.field(caller.as_context_mut(), 0)?;
            match field0 {
                Val::I64(i) => i64_to_u8(i),
                Val::I32(i) => i64_to_u8(i64::from(i)),
                other => bail!("expected integer in BoxedInt field, got {other:?}"),
            }
        }
        Val::AnyRef(None) => bail!("null element in host byte array"),
        other => bail!("unsupported host byte array element: {other:?}"),
    }
}

fn i64_to_u8(value: i64) -> Result<u8> {
    if (0..=255).contains(&value) {
        Ok(value as u8)
    } else {
        bail!("runtime byte out of range: {value}")
    }
}

fn decode_runtime_utf8_bytes(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).context("runtime string contained invalid UTF-8 bytes")
}

fn encode_runtime_string(
    caller: &mut Caller<'_, HostOutput>,
    string_array_ty: &ArrayType,
    value: &str,
) -> Result<Rooted<AnyRef>> {
    let allocator = ArrayRefPre::new(caller.as_context_mut(), string_array_ty.clone());
    let elems = value
        .as_bytes()
        .iter()
        .map(|b| Val::I32(i32::from(*b)))
        .collect::<Vec<_>>();
    let array = ArrayRef::new_fixed(caller.as_context_mut(), &allocator, &elems)?;
    Ok(array.to_anyref())
}

fn encode_runtime_string_array(
    caller: &mut Caller<'_, HostOutput>,
    string_array_ty: &ArrayType,
    runtime_array_ty: &ArrayType,
    values: &[String],
) -> Result<Rooted<AnyRef>> {
    let mut elems = Vec::with_capacity(values.len());
    for value in values {
        let value_ref = encode_runtime_string(caller, string_array_ty, value)?;
        elems.push(Val::AnyRef(Some(value_ref)));
    }
    let allocator = ArrayRefPre::new(caller.as_context_mut(), runtime_array_ty.clone());
    let array = ArrayRef::new_fixed(caller.as_context_mut(), &allocator, &elems)?;
    Ok(array.to_anyref())
}

fn host_exit_code_from_val(val: &Val) -> Result<i64> {
    match val {
        Val::I64(v) => Ok(*v),
        Val::I32(v) => Ok(i64::from(*v)),
        other => bail!("host.exit expected integer argument, got {other:?}"),
    }
}

pub fn build_engine() -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_reference_types(true);
    config.wasm_function_references(true);
    config.wasm_gc(true);
    Engine::new(&config).context("failed to create Wasmtime engine")
}

fn load_wasm_input(path: &str) -> Result<Vec<u8>> {
    let ext = Path::new(path).extension().and_then(|e| e.to_str());
    match ext {
        Some("tw") => {
            let wat = crate::cli::build::build_wat(path)?;
            Ok(wat.into_bytes())
        }
        Some("wat") | Some("wasm") => {
            std::fs::read(path).with_context(|| format!("failed to read input file '{}'", path))
        }
        Some(other) => bail!("unsupported input extension '.{other}' (use .tw, .wat, or .wasm)"),
        None => {
            std::fs::read(path).with_context(|| format!("failed to read input file '{}'", path))
        }
    }
}

/// Run a pre-compiled Wasmtime [`Module`] and return captured (stdout, stderr).
/// Useful for benchmarks where the engine and module are built once outside the
/// timed loop and only instantiation/execution is measured.
pub fn execute_module(engine: &Engine, module: &Module) -> Result<(String, String)> {
    let mut linker = Linker::new(engine);
    let host_imports = HostImportTypes::from_module(module)?;
    host_imports.define_all(&mut linker)?;

    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    let mut store = Store::new(
        engine,
        HostOutput {
            stdout: String::new(),
            stderr: String::new(),
            cwd,
            argv: vec![],
            env: HashMap::new(),
            exit_code: None,
        },
    );
    if let Err(err) = linker.instantiate(&mut store, module) {
        match store.data().exit_code {
            Some(0) => {}
            Some(code) => return Err(anyhow!("process exited with code {code}")),
            None => return Err(err).context("failed to instantiate/run Wasm module"),
        }
    }
    let out = store.data();
    Ok((out.stdout.clone(), out.stderr.clone()))
}

pub fn run_wasm_capture(path: &str) -> Result<(String, String)> {
    let wasm_input = load_wasm_input(path)?;
    let engine = build_engine()?;
    let module = Module::new(&engine, &wasm_input)
        .with_context(|| format!("failed to compile Wasm module from '{}'", path))?;

    let mut linker = Linker::new(&engine);
    let host_imports = HostImportTypes::from_module(&module)?;
    host_imports.define_all(&mut linker)?;

    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    let argv = vec![path.to_string()];
    let env = std::env::vars().collect::<HashMap<_, _>>();
    let mut store = Store::new(
        &engine,
        HostOutput {
            stdout: String::new(),
            stderr: String::new(),
            cwd,
            argv,
            env,
            exit_code: None,
        },
    );
    let instantiated = linker.instantiate(&mut store, &module);
    if let Err(err) = instantiated {
        match store.data().exit_code {
            Some(0) => {}
            Some(code) => {
                return Err(anyhow!("process exited with code {code}"));
            }
            None => {
                return Err(err)
                    .with_context(|| format!("failed to instantiate/run Wasm module '{}'", path));
            }
        }
    }

    let out = store.data();
    Ok((out.stdout.clone(), out.stderr.clone()))
}

pub fn run_wasm_file(path: &str) -> Result<()> {
    let (stdout, stderr) = run_wasm_capture(path)?;
    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{HostImportTypes, build_engine, decode_runtime_utf8_bytes};
    use wasmtime::Module;

    #[test]
    fn decode_runtime_utf8_bytes_accepts_valid_utf8() {
        let out = decode_runtime_utf8_bytes(b"hello".to_vec()).expect("valid utf8");
        assert_eq!(out, "hello");
    }

    #[test]
    fn decode_runtime_utf8_bytes_rejects_invalid_utf8() {
        let err = decode_runtime_utf8_bytes(vec![0xff]).expect_err("invalid utf8 should error");
        let msg = format!("{err:#}");
        assert!(msg.contains("invalid UTF-8"));
    }

    #[test]
    fn host_import_types_accepts_fs_import_surface() {
        let engine = build_engine().expect("build engine");
        let module = Module::new(
            &engine,
            r#"
                (module
                  (type $String (array (mut i8)))
                  (type $Array (array (mut anyref)))
                  (import "host" "read_file" (func (param (ref null $String)) (result (ref null $String))))
                  (import "host" "write_file" (func (param (ref null $String) (ref null $String))))
                  (import "host" "write_bytes" (func (param (ref null $String) (ref null $Array))))
                  (import "host" "mkdirp" (func (param (ref null $String))))
                  (import "host" "list_dir" (func (param (ref null $String)) (result (ref null $Array))))
                  (import "host" "exists" (func (param (ref null $String)) (result i32)))
                )
            "#,
        )
        .expect("compile host import module");

        let imports = HostImportTypes::from_module(&module).expect("collect host import types");
        assert!(imports.read_file.is_some());
        assert!(imports.write_file.is_some());
        assert!(imports.write_bytes.is_some());
        assert!(imports.mkdirp.is_some());
        assert!(imports.list_dir.is_some());
        assert!(imports.exists.is_some());
        assert!(imports.string_array_ty.is_some());
        assert!(imports.runtime_array_ty.is_some());
    }

    #[test]
    fn host_import_types_accepts_proc_and_stderr_import_surface() {
        let engine = build_engine().expect("build engine");
        let module = Module::new(
            &engine,
            r#"
                (module
                  (type $String (array (mut i8)))
                  (type $Array (array (mut anyref)))
                  (import "host" "eprint" (func (param (ref null $String))))
                  (import "host" "eprintln" (func (param (ref null $String))))
                  (import "host" "args" (func (result (ref null $Array))))
                  (import "host" "env" (func (param (ref null $String)) (result (ref null $Array))))
                  (import "host" "cwd" (func (result (ref null $String))))
                  (import "host" "exit" (func (param i64)))
                )
            "#,
        )
        .expect("compile host import module");

        let imports = HostImportTypes::from_module(&module).expect("collect host import types");
        assert!(imports.eprint.is_some());
        assert!(imports.eprintln.is_some());
        assert!(imports.args.is_some());
        assert!(imports.env.is_some());
        assert!(imports.cwd.is_some());
        assert!(imports.exit.is_some());
        assert!(imports.string_array_ty.is_some());
        assert!(imports.runtime_array_ty.is_some());
    }
}
