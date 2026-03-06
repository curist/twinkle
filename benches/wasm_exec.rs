use std::collections::HashMap;
use std::path::PathBuf;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use wasmtime::Module;

use twinkle::cli::run_wasm::{build_engine, execute_module};
use twinkle::codegen::emit::emit_user_module;
use twinkle::ir::lower_anf::lower_module;
use twinkle::ir::monomorphize::monomorphize;
use twinkle::opt::optimize_module;
use twinkle::runtime;
use twinkle::wasm::{emit::emit_wat, linker::link};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/run")
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn build_wasm_no_mono(file_path: &str) -> Vec<u8> {
    let (core_module, _) = twinkle::module::compile_entry(file_path).expect("compile failed");
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    wat::parse_str(&emit_wat(&linked)).expect("WAT parse failed")
}

fn build_wasm_with_mono(file_path: &str) -> Vec<u8> {
    let (core_module, _) = twinkle::module::compile_entry(file_path).expect("compile failed");
    let core_module = monomorphize(core_module);
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module(&optimized, &core_module.type_env, &HashMap::new());
    let mut modules = runtime::all_modules();
    modules.push(user_module);
    let linked = link(modules, None).expect("link failed");
    wat::parse_str(&emit_wat(&linked)).expect("WAT parse failed")
}

fn bench_wasm_exec(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");

    let cases = [("bench_generic", "bench_generic.tw")];

    for (label, file) in cases {
        let path = fixture(file);
        let bytes_no_mono = build_wasm_no_mono(&path);
        let bytes_with_mono = build_wasm_with_mono(&path);

        let module_no_mono = Module::new(&engine, &bytes_no_mono).expect("module (no mono)");
        let module_with_mono = Module::new(&engine, &bytes_with_mono).expect("module (with mono)");

        let mut group = c.benchmark_group(label);
        group.bench_function(BenchmarkId::new("exec", "no_mono"), |b| {
            b.iter(|| execute_module(&engine, &module_no_mono).expect("execution failed"))
        });
        group.bench_function(BenchmarkId::new("exec", "with_mono"), |b| {
            b.iter(|| execute_module(&engine, &module_with_mono).expect("execution failed"))
        });
        group.finish();
    }
}

criterion_group!(benches, bench_wasm_exec);
criterion_main!(benches);
