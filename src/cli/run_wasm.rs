use std::path::Path;

use anyhow::{Context, Result, anyhow, bail, ensure};
use wasmtime::{
    AnyRef, ArrayRef, ArrayRefPre, ArrayType, AsContext, AsContextMut, Caller, Config, Engine,
    ExternType, FuncType, HeapType, Linker, Module, Rooted, Store, Val, ValType,
};

#[derive(Default)]
struct HostOutput {
    stdout: String,
    stderr: String,
}

#[derive(Default)]
struct HostImportTypes {
    print: Option<FuncType>,
    println: Option<FuncType>,
    error: Option<FuncType>,
    f64_to_string: Option<FuncType>,
    string_array_ty: Option<ArrayType>,
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
                "print" | "println" | "error" => {
                    let candidate = concrete_array_from_func_param(&func_ty, 0)
                        .with_context(|| format!("invalid host import signature for {name}"))?;
                    merge_string_array_ty(&mut out.string_array_ty, candidate)?;
                    let slot = match name {
                        "print" => &mut out.print,
                        "println" => &mut out.println,
                        "error" => &mut out.error,
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

fn build_engine() -> Result<Engine> {
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

pub fn run_wasm_capture(path: &str) -> Result<(String, String)> {
    let wasm_input = load_wasm_input(path)?;
    let engine = build_engine()?;
    let module = Module::new(&engine, &wasm_input)
        .with_context(|| format!("failed to compile Wasm module from '{}'", path))?;

    let mut linker = Linker::new(&engine);
    let host_imports = HostImportTypes::from_module(&module)?;
    host_imports.define_all(&mut linker)?;

    let mut store = Store::new(&engine, HostOutput::default());
    linker
        .instantiate(&mut store, &module)
        .with_context(|| format!("failed to instantiate/run Wasm module '{}'", path))?;

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
    use super::decode_runtime_utf8_bytes;

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
}
