use std::collections::HashMap;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use wasmtime::Module;

use twinkle::cli::run_wasm::{build_engine, execute_module};
use twinkle::codegen::emit::{emit_user_module, emit_user_module_typed};
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

fn build_wasm_with_typed_closure(file_path: &str) -> Vec<u8> {
    let (core_module, _) = twinkle::module::compile_entry(file_path).expect("compile failed");
    let core_module = monomorphize(core_module);
    let anf = lower_module(&core_module);
    let optimized = optimize_module(anf);
    let user_module = emit_user_module_typed(&optimized, &core_module.type_env, &HashMap::new());
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

/// Three-way benchmark for the typed-closure specialization (Stage 9.6).
///
/// Uses bench_closure.tw (1000-element fold) to keep GC pressure reasonable while still
/// showing the relative dispatch overhead of each closure strategy:
///   no_mono          — universal anyref closures, no monomorphization
///   with_mono        — Stage 9.5: monomorphized direct calls, universal closure dispatch
///   with_typed_closure — Stage 9.5 + 9.6: typed call_ref, no arg boxing
fn bench_closure_exec(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");
    let path = fixture("bench_closure.tw");

    let bytes_no_mono = build_wasm_no_mono(&path);
    let bytes_with_mono = build_wasm_with_mono(&path);
    let bytes_typed = build_wasm_with_typed_closure(&path);

    let module_no_mono = Module::new(&engine, &bytes_no_mono).expect("module (no mono)");
    let module_with_mono = Module::new(&engine, &bytes_with_mono).expect("module (with mono)");
    let module_typed = Module::new(&engine, &bytes_typed).expect("module (typed closure)");

    let mut group = c.benchmark_group("bench_closure");
    group.bench_function(BenchmarkId::new("exec", "no_mono"), |b| {
        b.iter(|| execute_module(&engine, &module_no_mono).expect("execution failed"))
    });
    group.bench_function(BenchmarkId::new("exec", "with_mono"), |b| {
        b.iter(|| execute_module(&engine, &module_with_mono).expect("execution failed"))
    });
    group.bench_function(BenchmarkId::new("exec", "with_typed_closure"), |b| {
        b.iter(|| execute_module(&engine, &module_typed).expect("execution failed"))
    });
    group.finish();
}

/// Compare optimized collect lowering (Stage 10.2 builder path) against a
/// manual persistent push loop that still pays concat-per-append cost.
fn bench_collect_strategy_exec(c: &mut Criterion) {
    let engine = build_engine().expect("failed to build Wasmtime engine");

    let optimized_path = fixture("bench_collect_iterator.tw");
    let manual_path = fixture("bench_manual_push_iterator.tw");

    // Use monomorphized pipeline for both to reduce unrelated polymorphism noise.
    let optimized_bytes = build_wasm_with_mono(&optimized_path);
    let manual_bytes = build_wasm_with_mono(&manual_path);

    let optimized_module = Module::new(&engine, &optimized_bytes).expect("module (collect)");
    let manual_module = Module::new(&engine, &manual_bytes).expect("module (manual push)");

    let mut group = c.benchmark_group("bench_collect_strategy");
    group.bench_function(BenchmarkId::new("exec", "collect_builder"), |b| {
        b.iter(|| execute_module(&engine, &optimized_module).expect("execution failed"))
    });
    group.bench_function(BenchmarkId::new("exec", "manual_push"), |b| {
        b.iter(|| execute_module(&engine, &manual_module).expect("execution failed"))
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_wasm_exec,
    bench_closure_exec,
    bench_collect_strategy_exec
);
criterion_main!(benches);
